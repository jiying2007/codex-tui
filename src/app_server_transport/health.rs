//! Connection-local write fence. Cancellation must not make a partial frame retryable.
use anyhow::{Context, Result};
use std::{future::Future, time::Duration};

#[derive(Default)]
pub(super) struct TransportHealth {
    fault: Option<&'static str>,
}

impl TransportHealth {
    pub(super) fn is_quarantined(&self) -> bool {
        self.fault.is_some()
    }

    pub(super) fn check(&self) -> Result<()> {
        if let Some(reason) = self.fault {
            anyhow::bail!("App Server transport quarantined: {reason}");
        }
        Ok(())
    }

    pub(super) fn observe_read<T>(&mut self, result: &Result<Option<T>>) {
        self.fault = match result {
            Err(_) => Some("wire read or JSON decoding failed"),
            Ok(None) => Some("peer closed the connection"),
            Ok(Some(_)) => self.fault,
        };
    }

    pub(super) async fn write(
        &mut self,
        deadline: Duration,
        future: impl Future<Output = Result<()>>,
    ) -> Result<()> {
        self.check()?;
        // Persist before the first write poll. An outer RPC deadline may drop
        // this future, bypassing all code after await; only success clears it.
        self.fault = Some("outbound write incomplete; delivery unknown; do not replay");
        let result = tokio::time::timeout(deadline, future)
            .await
            .context("App Server outbound write deadline exceeded; delivery unknown")?;
        if result.is_ok() {
            self.fault = None;
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::FutureExt;
    use serde_json::json;
    use tokio::io::{AsyncReadExt, BufWriter, duplex};

    #[tokio::test]
    async fn cancelled_partial_stdio_write_fences_later_bytes() {
        let mut health = TransportHealth::default();
        let (client, mut peer) = duplex(8);
        let mut writer = BufWriter::with_capacity(16, client);
        let value = json!({"id":1,"method":"fixture/write","params":"x".repeat(256)});
        assert!(
            health
                .write(
                    Duration::from_secs(1),
                    crate::app_server_transport::write_stdio_json(&mut writer, &value)
                )
                .now_or_never()
                .is_none()
        );
        let mut prefix = [0; 8];
        peer.read_exact(&mut prefix).await.unwrap();
        assert_eq!(&prefix, &serde_json::to_vec(&value).unwrap()[..8]);
        assert!(health.is_quarantined());
        let later = json!({"id":2,"method":"fixture/must-not-send"});
        let error = health
            .write(
                Duration::from_secs(1),
                crate::app_server_transport::write_stdio_json(&mut writer, &later),
            )
            .await
            .unwrap_err();
        assert!(error.to_string().contains("delivery unknown"));
        assert!(peer.read(&mut prefix).now_or_never().is_none());
    }

    #[tokio::test]
    async fn flush_cancellation_also_fences_buffered_output() {
        let mut health = TransportHealth::default();
        let (client, mut peer) = duplex(4);
        let mut writer = BufWriter::with_capacity(1024, client);
        let value = json!({"id":1,"method":"fixture/flush"});
        assert!(
            health
                .write(
                    Duration::from_secs(1),
                    crate::app_server_transport::write_stdio_json(&mut writer, &value)
                )
                .now_or_never()
                .is_none()
        );
        assert!(!writer.buffer().is_empty());
        let mut prefix = [0; 4];
        peer.read_exact(&mut prefix).await.unwrap();
        assert!(health.check().is_err());
        assert!(
            health
                .write(Duration::from_secs(1), async {
                    panic!("must not poll retry")
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn unpolled_cancellation_and_success_do_not_poison_connection() {
        let mut health = TransportHealth::default();
        let unpolled = health.write(Duration::from_secs(1), async { Ok(()) });
        drop(unpolled);
        assert!(!health.is_quarantined());
        health
            .write(Duration::from_secs(1), async { Ok(()) })
            .await
            .unwrap();
        assert!(!health.is_quarantined());
    }

    #[tokio::test]
    async fn local_deadline_and_write_error_remain_quarantined() {
        let mut health = TransportHealth::default();
        let error = health
            .write(Duration::from_millis(1), std::future::pending())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("deadline"));
        assert!(health.check().is_err());
        let mut health = TransportHealth::default();
        let (client, peer) = duplex(16);
        drop(peer);
        let mut writer = BufWriter::new(client);
        assert!(
            health
                .write(
                    Duration::from_secs(1),
                    crate::app_server_transport::write_stdio_json(&mut writer, &json!({"id":1}))
                )
                .await
                .is_err()
        );
        assert!(health.check().is_err());
    }
}
