# Product

<!-- impeccable:product-schema 1 -->

## Platform

Native macOS desktop. The installed design-skill schema has no macOS value; this product is neither a web app nor an iOS app.

## Stack

Rust core and CLI, with a native GPUI GUI and a reusable component library. The specific component-library version and GPUI dependency alignment remain to be validated before implementation.

## Users

General Mac users and developers/power users are equally important in the first GUI release. Both need to understand where space went, inspect the responsible files and directories, and choose what to remove.

## Product Purpose

Make local disk usage understandable and actionable without compromising scan performance, filesystem correctness, or cleanup safety.

## Operating Context

A local macOS utility built on the existing `clrs-core` scanner. The CLI remains supported, including its JSON output. The GUI should call the Rust library directly, not spawn the CLI or parse its JSON.

## Capabilities and Constraints

- The user explicitly selected full file-management scope for the GUI, including choosing arbitrary files and folders for Trash. This is a requested capability, not an existing core API.
- Existing capabilities include allocated-size scanning, immutable scan trees, progress and cancellation, cached scans, cleanup recommendations, and rule-based cleanup.
- The scan does not follow symlinks; hard links are charged once. Unreadable locations produce recorded issues rather than aborting the scan.
- Existing cleanup can move a rule finding to Trash or run that rule's command. These actions must not be presented as equivalent or equally reversible.
- The GUI must preserve the scanner's correctness and performance and reuse its implementation. A narrow shared cleanup safety API will need to be planned for arbitrary selections; no second scanner or GUI-only deletion implementation.
- Planning is limited to three implementation phases. No GUI implementation is authorized by the planning task itself.
- Existing project rules require end-to-end tests and prohibit real cleanup actions in tests. GUI test coverage needs a compatible dry-run strategy; any broader test-policy change remains explicit.

## Brand Commitments

Professional, carefully designed presentation; understandable navigation and complete screens/states. Restrained playfulness is welcome where it improves the experience. Avoid generic generated-dashboard aesthetics and decorative complexity.

## Evidence on Hand

The current working checkout has a functioning CLI/core and a GUI Cargo manifest. Repository details, research sources, proposed design decisions, and acceptance gates belong in `docs/GUI_IMPLEMENTATION_PLAN.md`; proposals there are not approved visual specifications.

## Product Principles

- Let people inspect before they act.
- Make filesystem uncertainty and incomplete scans visible.
- Preserve familiar macOS interaction conventions.
- Offer progressive detail rather than separate beginner and expert products.
- Do not trade safety or responsiveness for visual novelty.

## Open Decisions

- Deployment minimum: current GPUI Kit installation guidance lists macOS 15+; the product's supported OS floor has not been approved.
- Component/GPUI release pairing and Rust toolchain baseline, subject to a compile-and-accessibility spike.
- Distribution identity, signing/notarization credentials, and release channel.
- Final visual direction and implementation-plan approval.
