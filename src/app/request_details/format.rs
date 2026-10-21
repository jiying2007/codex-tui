use crate::{
    conversation::{ApprovalContext, InteractiveRequest, InteractiveRequestKind},
    i18n::{UiLanguage, pick},
    text::{sanitize_inline, sanitize_multiline},
};

fn approval_scope_label(kind: &str, language: UiLanguage) -> &'static str {
    match (kind, language) {
        ("grant-root", UiLanguage::SimplifiedChinese) => "授权根目录",
        ("network", UiLanguage::SimplifiedChinese) => "网络权限",
        ("network-host", UiLanguage::SimplifiedChinese) => "网络主机",
        ("filesystem", UiLanguage::SimplifiedChinese) => "文件权限",
        ("filesystem-read", UiLanguage::SimplifiedChinese) => "文件读取",
        ("filesystem-write", UiLanguage::SimplifiedChinese) => "文件写入",
        ("permission-category", UiLanguage::SimplifiedChinese) => "其他权限类别",
        ("permission-detail", UiLanguage::SimplifiedChinese) => "未识别权限字段",
        ("grant-root", _) => "grant root",
        ("network", _) => "network",
        ("network-host", _) => "network host",
        ("filesystem", _) => "filesystem",
        ("filesystem-read", _) => "filesystem read",
        ("filesystem-write", _) => "filesystem write",
        ("permission-detail", _) => "unrecognized permission field",
        _ => "other permission category",
    }
}

fn append_approval_scope(
    lines: &mut Vec<String>,
    context: &ApprovalContext,
    language: UiLanguage,
    sanitize: fn(&str) -> String,
) {
    for (kind, value) in context.visible_scope() {
        let mut value = sanitize(&value);
        if matches!(kind.as_str(), "permission-category" | "permission-detail") {
            value.push_str(pick(language, " · details hidden", " · 详情已隐藏"));
        }
        lines.push(format!(
            "{}: {value}",
            approval_scope_label(&kind, language)
        ));
    }
}

/// Summary and detail views share the same known scope. Only details preserve
/// safe newlines and enumerate all input questions; opaque scope stays hidden.
pub(crate) fn request_lines(
    request: &InteractiveRequest,
    language: UiLanguage,
    detailed: bool,
) -> Vec<String> {
    let sanitize: fn(&str) -> String = if detailed {
        sanitize_multiline
    } else {
        sanitize_inline
    };
    let mut lines = vec![
        pick(
            language,
            if detailed {
                "NEEDS YOU"
            } else {
                "NEEDS YOU · d full details"
            },
            if detailed {
                "需要你处理"
            } else {
                "需要你处理 · d 完整详情"
            },
        )
        .to_owned(),
    ];
    match &request.kind {
        InteractiveRequestKind::CommandApproval {
            context,
            command,
            cwd,
            reason,
        } => {
            lines.push(format!(
                "{}: {}",
                pick(language, "Command approval", "命令审批"),
                sanitize(command)
            ));
            if !cwd.is_empty() {
                lines.push(format!("cwd: {}", sanitize(cwd)));
            }
            if let Some(reason) = reason {
                lines.push(format!(
                    "{}: {}",
                    pick(language, "reason", "原因"),
                    sanitize(reason)
                ));
            }
            append_approval_scope(&mut lines, context, language, sanitize);
            if !detailed {
                lines.push(
                    pick(
                        language,
                        "y accept · n decline · c cancel",
                        "y 接受 · n 拒绝 · c 取消",
                    )
                    .into(),
                );
            }
        }
        InteractiveRequestKind::FileChangeApproval { context, reason } => {
            lines.push(pick(language, "File change approval", "文件变更审批").into());
            if let Some(reason) = reason {
                lines.push(format!(
                    "{}: {}",
                    pick(language, "reason", "原因"),
                    sanitize(reason)
                ));
            }
            append_approval_scope(&mut lines, context, language, sanitize);
            if !detailed {
                lines.push(
                    pick(
                        language,
                        "y accept · n decline · c cancel",
                        "y 接受 · n 拒绝 · c 取消",
                    )
                    .into(),
                );
            }
        }
        InteractiveRequestKind::PermissionsApproval {
            context,
            reason,
            network_requested,
            filesystem_requested,
        } => {
            lines.push(format!(
                "{}: network={network_requested} filesystem={filesystem_requested}",
                pick(language, "Permission request", "权限请求")
            ));
            if let Some(reason) = reason {
                lines.push(format!(
                    "{}: {}",
                    pick(language, "reason", "原因"),
                    sanitize(reason)
                ));
            }
            append_approval_scope(&mut lines, context, language, sanitize);
            if !detailed {
                lines.push(
                    pick(
                        language,
                        "y grant for this turn · n/c decline",
                        "y 本轮授权 · n/c 拒绝",
                    )
                    .into(),
                );
            }
        }
        InteractiveRequestKind::UserInput { questions } => {
            lines.push(if language.is_simplified_chinese() {
                format!("需要用户输入: {} 个问题", questions.len())
            } else {
                format!("User input requested: {} question(s)", questions.len())
            });
            for question in questions
                .iter()
                .take(if detailed { questions.len() } else { 1 })
            {
                lines.push(format!(
                    "{}: {}",
                    sanitize(&question.header),
                    sanitize(&question.question)
                ));
                if !question.options.is_empty() {
                    if detailed {
                        lines.push(format!("{}:", pick(language, "options", "选项")));
                        lines.extend(
                            question
                                .options
                                .iter()
                                .map(|option| format!("  {}", sanitize(option))),
                        );
                    } else {
                        lines.push(format!(
                            "{}: {}",
                            pick(language, "options", "选项"),
                            question
                                .options
                                .iter()
                                .map(|option| sanitize(option))
                                .collect::<Vec<_>>()
                                .join(", ")
                        ));
                    }
                }
            }
            if !detailed {
                lines.push(pick(language, "i answer", "i 回答").into());
            }
        }
    }
    lines
}
