# Getting started and Desktop lifecycle

## Purpose

HiRoute Desktop lets a local user connect model sources, publish routing plans,
configure coding Agents, and inspect execution without an embedded chat Agent.
This specification covers the application shell, installation, and upgrade handoff.

## Requirements

### Requirement: Service health and setup progress are separate facts

Desktop MUST display the actual local service state: starting, available,
unavailable, or incompatible. A successful service read establishes health;
a retained snapshot after a failed refresh does not. Source, plan, and Agent
queries that are loading or empty MUST NOT turn a healthy service indicator into
a setup warning. The home page owns onboarding progress separately.

Sources, plans, model access, and task collaboration MUST be configurable
independently. A user may start with a single fixed model, add resources later,
or enable collaboration while retaining the main Agent's existing model provider.
Home MUST show current configured resources, recent sessions, active delegated
work, and available valuation with its completeness. Empty usage is an onboarding
state, never invented savings or a chart of fabricated zeros.

#### Scenario: Return while a resource query fails

- **WHEN** service status succeeds but a model or history query is pending or fails
- **THEN** Desktop MUST retain the correct service state, identify the affected read,
  and offer a retry without overwriting edits or claiming the resource is absent.

### Requirement: Preferences preserve the current interaction

The production shell MUST support English and Chinese, light and dark themes,
and 100%, 150%, and 200% text sizing. Without an explicit language preference,
Chinese or English follows the system locale; other locales use English. Explicit
choices and restoring system-following behavior MUST survive restart. Text size
starts at 100% without a saved choice and MUST NOT derive from device pixel ratio.

Changing locale, theme, text size, navigation width, or a supported window size
MUST preserve non-secret edits, selections, and the active Operation identity.
Headings, controls, supporting text, empty states, and dialogs MUST scale together;
primary actions remain reachable. Keyboard navigation, visible focus, readable
labels, dialog focus/return, and status indications beyond color MUST work.
Non-submitting dialogs support close and Escape; unsaved-loss confirmation occurs
once. A submission may prevent closing only for its actual pending phase, with an
explanation and a bounded recovery path after transport failure.

The effective macOS theme MUST extend to native window chrome. Native traffic
lights, title-area dragging, and the user's title-bar double-click setting remain
usable without a conflicting duplicate title layer. HTTP(S) product links open
through a main-window native browser command restricted to valid HTTP(S) URLs;
file, shell, custom schemes, and URLs without authority MUST be rejected. Browser
previews retain ordinary link behavior.

#### Scenario: Change appearance during a failed save

- **WHEN** a user changes language or text size after a submission/read failure
- **THEN** the same input and Operation association MUST remain available, with
  accessible retry/close controls and no new submission caused by the preference.

### Requirement: The shell reports the current Operation once

Shared submission feedback MUST follow server Operation identity and sequence.
Pending feedback names the action and object and uses an indeterminate indicator
unless measured progress exists. Succeeded, rolled back, blocked, and uncertain
outcomes replace that feedback at the same location. A new Operation replaces the
previous one; stale queries cannot overwrite a newer identity or confirmed terminal
state. A short read delay or transient failure stays neutral while checking the
original identity. Persistent uncertainty exposes one result-to-be-confirmed state
and a query action. Closing feedback hides it without cancelling server work;
success feedback settles after a short display period.

#### Scenario: Two saves overlap in observation

- **WHEN** a late status response for the first save arrives after the second starts
- **THEN** it MUST NOT replace the second save's feedback or trigger a replay.

### Requirement: Information and confirmation match the user's decision

Desktop MUST prioritize the current action and expose cost, permission scope,
completeness, and recovery information before they affect a decision. Infrequent
technical detail may be expandable but core configuration and risk cannot be
hidden. Expandable regions need a discoverable row, localized state, direction,
keyboard support, and correct focus; actions inside them do not toggle the row.
A short standalone field, including a plan's stable model invocation name, remains
visible. Searching options MUST NOT change selected model identity or order.

Save, draft, ordinary publication, and compatibility checks use a clear action
button without a second confirmation of the same intent. Destructive confirmations
identify target and consequence. Internal revisions and digests remain enforced
by the backend without requiring a user to inspect them.

Brand assets MUST be bundled, offline-available, consistent across source, Agent,
Worker, task, and session views, and paired with a readable name. Known Agents use
their actual logos. Provider branding cannot imply a model-specific identity;
unknown objects use a neutral fallback rather than a fabricated brand.

### Requirement: Upgrade preparation keeps the running application available

Desktop MUST select a newer stable release for the current architecture from the
trusted website release catalog. Download path, byte count, SHA-256, package
inventory, component architecture, and existing signature identity MUST be checked
by native code. A self-signed package's origin is established by HTTPS and the
catalog digest; its signature proves package integrity, not Developer ID identity.
WebView actions cannot supply an arbitrary URL, installation path, or trust identity.

The current catalog is the website's `releases.json`; its native reader selects the
actual local architecture rather than accepting a WebView-provided download target.

Downloading and validating MUST leave the service running. Upgrade staging is
native-owned and private. Standard group-writable installation ancestors such as
macOS `/Applications` MUST NOT be rejected using private-data-directory rules.
An unwritable destination fails explicitly without automatic elevation.

#### Scenario: Invalid or changed package

- **WHEN** the mounted App, inventory, digest, architecture, or identity differs
  from the selected release or changes after preparation
- **THEN** installation MUST fail before replacement; existing service/data remain
  available and the failure identifies the relevant stage.

### Requirement: Replacement requires an explicit orderly-stop handoff

The installer MUST originate from the current complete App, preflight, and report
ready before the protected launcher handoff commits replacement. Admission drains
active requests, durable workspace occupancy, and Worker cleanup. The user can
cancel while waiting; cancellation restores admission. A timeout or merely sending
a stop signal does not prove exit and MUST NOT authorize replacement.

Replacement proceeds only after the owned daemon and Desktop have actually exited,
the original host lock is acquired, and package identities are rechecked. Loss of
the parent before explicit commit does not authorize installation. Failures before
replacement restore admission or restart the verified old service through its
normal entry. Uncertain exit remains stop-pending; failed recovery requires an
explicit restart indication. Stable errors distinguish handoff, preflight, commit
wait, process exit, and replacement failures.

#### Scenario: Cancel while a Worker is active

- **WHEN** an upgrade waits for accepted work and the user cancels
- **THEN** work MUST NOT be killed or resubmitted, and normal admission resumes.

### Requirement: Installation and data recovery use verified complete packages

Automatic upgrade and manual overwrite MUST enter the same new-daemon storage
validation, backup, and supported migration path before business access opens.
[Storage recovery](../local-data-and-diagnostics/spec.md) owns its data contract.
Desktop MUST expose backup location and recovery instructions even when daemon
startup fails. Recovery identifies the complete matching old package, preserves
current directories before restoring the whole backup group, and warns that data
created after the backup will be discarded. No automatic restoration follows a
completed migration.

A migration requiring a readable matching old package MUST bind its actual local
identity before changing data. Manual installation without that binding exposes
native selection of the old App; failed selection changes no business data.
Rebuilding the native session invalidates old confirmations and pending approvals.
Cleanup for a completed subscription check stays bound to its original session:
closing a window still releases its resources, but callbacks cannot deliver a result
or consume confirmation in a replacement session.

macOS packages MUST be architecture-consistent for Apple Silicon or Intel and
include Desktop, daemon, CLI, frontend, CPA, and the current catalog. The minimum
OS must satisfy bundled dependencies. The CLI remains usable through its documented
full path without a development toolchain. Replacing an App in the same location
preserves data and managed entry points; moving it requires the normal connection
repair flow. Uninstall first restores managed Agent access, disables login items,
and exits. Deleting the App alone preserves data; optional cleanup cannot delete
projects or native Agent credentials. Package construction alone is not signing,
notarization, native installation, or live-Agent evidence.

### Requirement: New-model trials compose the existing protected workflow

Home MUST show at most six featured model entries with origin, availability and
version disclosure; without featured entries it offers unused executable or
connectable embedded models. Models needing a newer client cannot start a trial.
Home and routing creation open one flow for trial model, in-place connection,
Primary model, decision connection, selected Agent/presets and review.

A registered connection must observe the selected model before continuing. Protected
input/check/save and native subscription import retain their existing authority.
Primary is prefilled only from a recorded unambiguous fixed-model resolution; a
native alias does not identify a model version. Decision connections can be reused
or created in place; this flow offers no heuristic classification. The editable
review begins at Trial 0.90 with explicit low-progress/corrected-result competence
criteria, both model groups, saved connections and the proposed mapping scope.

Confirmation saves the ordinary draft, publishes it after its exact save and model
facts are confirmed, then uses normal Agent Preview/Apply. Mapping only changes the
selected presets/default; unrelated mappings, permissions and collaboration stay
intact. Flow IDs are retained in memory across retries without new daemon trial
state. Uncertain replies reconcile the same operation and exact resulting revision,
never the preceding successful operation. Completed connections, drafts and Plans
remain visible after later failures. A published Plan can be mapped from Routing
after reload without recreating it.

After this flow confirms that the Agent mapping applied successfully, it SHALL wait for matching Plan activity while the dialog and window remain visible. It SHALL query the existing sessions view from that confirmation time at a five-second cadence, skipping an in-flight read. The first matching candidate observed by the flow SHALL remain selectable through "Watch this session", which opens its live routing timeline only on user action. A Plan match SHALL NOT be described as proof of the Agent identity or a newly created, chronologically first session. Closing or hiding the flow stops reads; showing it resumes them. A read failure SHALL offer retry while retaining the model-performance and session-list actions.

#### Scenario: Publication succeeds but its reply is lost

- **WHEN** saving a trial draft succeeds and the subsequent publication reply is lost
- **THEN** refresh MUST verify the publication operation and exact Plan revision,
  without repeating either mutation or treating the draft save as publication;
  Agent mapping remains a separate reviewed action.

#### Scenario: Post-mapping session handoff

- **WHEN** the mapping is confirmed and a subsequent session read finds activity using the published Plan
- **THEN** the flow offers Watch this session without automatic navigation; old activity before confirmation is excluded, and closing the dialog stops waiting.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/140#issuecomment-6235502571

### Requirement: Home lists routing suggestions that need attention

The Desktop Home "Needs your attention" section SHALL list each current level suggestion for difficulty-routing Plans and two-group branches: step up, step down, create a division draft, or return to previous routing. Suggestions SHALL come from the same backend computation as the model performance view. Each item SHALL name the Plan and its published trust level, state the facts behind the suggestion (scored stages, scoring coverage, contributing sessions, competence rate, protection count and, for a level change, the replay share at the suggested level), carry the version disclosure and the actual suggestion evidence window/cohort of the model performance view when they apply, and offer two actions. "View evidence" opens the Plan's model performance view. The primary action opens the existing draft, division-draft or Agent settings Preview/Apply. Neither action SHALL change routing until the user publishes or applies. An item SHALL disappear when its condition no longer holds for the active published revision. While Home is not shown, navigation SHALL mark that suggestions exist. Suggestions SHALL be presented as optional; keeping the current level SHALL remain a valid outcome, and trust levels SHALL NOT be presented as onboarding progress or completion. When no suggestion applies, the section SHALL say so without inventing progress. A failed suggestion read SHALL be shown as a retryable read failure and SHALL NOT be shown as an absence of suggestions.


Suggestion failures SHALL be isolated per Plan: an oversized or unavailable Plan appears as a retryable unavailable item alongside other valid suggestions. The response SHALL provide suggestions and unavailable arrays, with unavailable entries naming the Plan and a stable scope_too_large, unavailable or stale reason. Suggested branches SHALL expose their counts, cohort, protection count, identity-stability disclosure and replay at the suggested threshold without returning full report rows or session lists. A failed whole query remains a retryable failure. Desktop SHALL refresh on Home display, window visibility and the user's own publish/apply actions, and otherwise every five minutes while visible; navigation and Plan markers reuse this read. Cached evidence SHALL still validate reader authority and all relevant control, observation, Worker and retention changes, including expiry with time.

#### Scenario: Two Plans, two directions

- **WHEN** one Plan qualifies to step up and another to step down
- **THEN** Home lists both items with their facts, and opening the step-up draft changes nothing until it is published.

#### Scenario: Condition resolved

- **WHEN** the user publishes the suggested level for a Plan
- **THEN** that Plan's item disappears, and no new suggestion appears for it until the new revision reaches the minimum scored stages.

#### Scenario: Read failure

- **WHEN** the suggestion query fails while other Home reads succeed
- **THEN** the section reports the failed read with a retry, and the other Home content stays visible.

#### Scenario: One Plan exceeds the read limit

- **WHEN** one enabled Plan exceeds its read bound while another has an available suggestion
- **THEN** Home shows the healthy Plan's suggestion and a retryable failure for the oversized Plan, without calling the result empty or disabling the healthy Plan's actions.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/140#issuecomment-6236472105

## Implementation references

- [Native updater](../../../apps/desktop/src-tauri/src/updates.rs) and
  [installer handoff tests](../../../apps/desktop/src-tauri/src/updates/installer/control/tests.rs).
- [Desktop preferences](../../../apps/desktop/src/ui/preferences.ts) and
  [product shell](../../../apps/desktop/src/product).
- [Release identity](../../../crates/host-runtime/src/releases.rs) and
  [macOS installation guide](../../../docs/macos-installation.md).
