# CLI request input ownership

`src/request_input.rs` owns selection of `--request-stdin` or `--request-file PATH`,
bounded byte reads and strict UTF-8 JSON decoding. A request file must be regular;
opening a FIFO does not wait for a writer. Relative paths use the caller's working
directory. Prefix option-shaped filenames with `./`. Exactly one source is accepted;
repeated sources, mixed sources and mixtures with human options fail with
`INVALID_ARGUMENTS` before Local Control access. A file never reads stdin.

The parser rejects duplicate object members at every nesting level (including escaped
aliases of the same key) and trailing JSON. `src/request_input/fields.rs` audits the
typed request's declared object fields, including skipped optional fields and tagged
unit variants; maps/JSON Values keep free keys and typed map values remain checked.
Serde's existing positional struct arrays remain accepted and checked in field order.
Current request DTOs have matching Serialize/Deserialize names without aliases or
flatten; future asymmetric codecs require an explicit adapter and regression tests.
This CLI boundary must not tighten persisted Domain recovery readers. Command owners
then select the permitted schema/intent and retain semantic checks:

- `control_plane.rs`: typed management requests and Preview/Apply schema unions.
- `observation.rs`: sessions, timeline, Facts, value and plan-quality intents.
- `lib.rs`: Operation idempotency lookup; legacy positional command codecs.
- `worker/input.rs`: dependency selection only; `--file` remains prompt text.
- `tasks.rs` and `work_plans.rs`: staged collaboration selectors and grant separation;
  adding a source does not promote these commands to the public command registry.

Keep each existing limit: 1 MiB by default, 64 KiB for Operation idempotency lookup
and dependency selection, 4 KiB for staged WorkPlan listing. Protected secret and
capability readers are independent; JSON/file selectors do not grant authority.
The shared reader does not execute a shell, alter requests or choose a business schema.

`sessions receipt RECEIPT_ID` continues to serialize `SessionLookupV1` and read the v1
RoutingReceipt. Both JSON sources accept only an `ObservationReadRequestV2` with the
`facts` intent and its logical RequestId from a v2 timeline, through the existing
`GetRoutingReceipt` operation. The response is the v2 Facts page. Wrong intents fail
locally. Do not translate a SessionId or ReceiptId into a RequestId.

Positional resource codecs exclude empty and option-shaped values. Unknown options
must be usage errors before a lookup; a failed resource search cannot stand in for
argument validation. `src/args.rs` preserves the request-file path as an option value
while extracting global flags. Follow `tests/request_input.rs` for actual binary argv
regressions and the owner codec tests for typed projections. Generated help/channel
metadata belongs to application-api; regenerate CLI and Product E2E contracts together.
