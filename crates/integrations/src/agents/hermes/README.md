# Hermes main-model integration

The shared Agent settings transaction owns admission, grants, Operations and
recovery. Hermes leaves own native configuration and resources:

- [Target resolution](../hermes_target.rs) binds one explicit canonical effective
  profile directory to its context and accepted Operation. Daemon environment does
  not select the profile.
- [Configuration](../hermes_config.rs) owns the default main model/provider and
  context controls, one per-Plan provider and the plugin's enabled settings.
  Conditional restore preserves unrelated configuration and refuses changed owned
  fields. Native source providers, credentials, channels and history remain native.
- [Plugin effects](../hermes_plugin_effects.rs) install the bundled dependency-free
  module/manifest in the same native tail. Identical borrowed files retain their
  ownership; foreign content is a conflict. Installed files do not prove loading
  in an already-running Hermes process.
- [Support inspection](../hermes_support.rs) reads the selected launcher's native
  plugin/config/context interfaces and protocol SDK. Responses requires OpenAI
  Responses with extra headers; Chat Completions independently requires OpenAI
  Chat Completions with extra headers; Messages requires Anthropic Messages with extra
  headers. Chat also requires route-aware reasoning replay for the local alias:
  Hermes v0.21.7 is the minimum verified Chat release. Native 0.19.1 strips that
  state and cannot configure Chat, while its verified Responses/Messages component
  support remains separate. Version text alone does
  not establish support. Missing interfaces block new connection writes; passive
  source discovery, independent Skill preparation and safe owned restore use their
  own capabilities.

The Chat release baseline is v0.21.7 (`71d34bad1045610252eda02ee79549436896e784`);
v0.22.1 contains the same required replay interfaces. Source distributions may
report `0.0.0`, so admission inspects the selected launcher's replay interfaces
and SDK rather than accepting a version label. Missing support blocks new Chat
connection writes. HiRoute does not upgrade Hermes automatically; safe owned
restore, passive scan and independent management preparation remain available.
- [Passive sources](../hermes_sources.rs) read the selected profile's static
  providers/default and literal profile environment. Helpers, rotating custom
  credential pools and unresolved composition remain unavailable. Unrelated native
  pools do not replace a selected custom provider's key. Managed HiRoute providers
  cannot become recursive upstream sources.

The installed management Skill is independent of the model connection. Prepare it
with `management_skill: configure` and `model: keep`; restore either facet without
restoring the other. Hermes has no Worker/collaboration setup in this integration.

Select `chat_completions`, `responses` or `messages` explicitly for the managed main model. Native SDK support and upstream request expressibility are separate checks. In particular, native Responses encrypted reasoning/summary controls cannot be dropped when the upstream only accepts Chat Completions; use native Chat for a qualified Chat source. Chat uses the existing OpenAI SDK and does not require the optional Anthropic SDK. Existing saved transports are not changed automatically.

Normal connection Apply prepares the provider and adapter together. Reload native
plugins or start a new native process when required. A channel/session `/model`
choice can retain a different model; use native selection controls in that channel
to adopt the managed default. Static channel selections are reported without
exposing channel IDs; unobserved live session selection remains unknown. Auxiliary
model slots stay under native control.

The plugin matches the exact saved provider definition, alias, native address and
transport; it carries the token and the actual native session ID through existing
SDK headers. It preserves unrelated request parameters and headers. Invalid or
missing IDs do not remove authorization. Gateway treats identity as Hermes only
for a digest-bound managed consumer, then scopes it to that connection; equal
content never supplies missing Hermes identity.

Source import is separate from model connection. The shared V2 scan/prepare flow
groups equal native scope/provider/address/protocol/authentication branches, then
offers native declarations and exact qualified catalog models. CLI guidance and
Desktop initially select all currently selectable models; Apply always contains
an explicit nonempty model-reference array. A refresh never expands saved models.

See [the Agent contract](../../../../../issue-spec/specs/agent-connections/spec.md),
[discovery contract](../../../../../issue-spec/specs/model-discovery/spec.md),
[source tests](../hermes_sources_tests.rs), and the
[public CLI journey](../../../../daemon/tests/support/connection_point_product.py).
