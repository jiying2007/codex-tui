# ADR-006: Attention is orthogonal to workflow stage

Date: 2026-09-29
Status: Accepted

## Decision

Planning workflow uses Inbox -> Ready -> Working -> Review -> Done.

Needs You is not a workflow stage. It is an attention overlay/filter/virtual lane.

A WorkCard can therefore be Working + Needs You or Review + Needs You.

## Rationale

Approval, user input, blocked goals, pipeline failure and unseen review findings are reasons for human attention, not replacements for the underlying work stage.

Mixing attention into the workflow destroys useful state and makes board transitions ambiguous.

## Consequence

Stage derivation and attention derivation are separate pure rules and are tested independently.