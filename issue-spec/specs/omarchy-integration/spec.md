# Omarchy integration

## Purpose

On an Omarchy desktop, HiRoute can add its own launcher entries, a top-bar
indicator and desktop notifications, so routed Codex and Claude Code sessions are as
easy to start and observe as Omarchy's native agent entries. The integration is
optional, current-user only and limited to files it owns. It never takes over
Omarchy's native entries, account selection or configuration.

## Requirements

### Requirement: Omarchy integration is opt-in and owns only its user-level files

HiRoute SHALL provide public CLI commands, shipped with the Linux release, to enable,
inspect and disable an Omarchy integration for the current user. Enable SHALL require
a detected Omarchy environment and a current-user HiRoute installation; otherwise it
SHALL fail with an explicit reason before any write. The integration SHALL write only
user-level files that it creates and records as owned: launcher entries for routed
Codex, routed Claude Code and the console; the top-bar indicator plugin and its bar
placement; and its own notification preferences. It SHALL NOT modify Omarchy-owned
files, native agent configuration, Agent connections, routing Plans, or Omarchy's
account and usage state. Status SHALL report each owned item as present, missing or
changed by the user, and whether each routed entry is currently launchable. Disable
SHALL remove unchanged owned items, keep and report items the user changed, and leave
Agent connections and Plans unchanged. Enabling again or upgrading HiRoute SHALL
refresh unchanged owned items to the installed release without duplicating entries,
and repeated enable or disable SHALL be idempotent. Enabling SHALL NOT start, stop or
restart the HiRoute service, and SHALL NOT make the integration a default for any
Agent. Its only service configuration change is the consented loopback console
initialisation described in the Omarchy surfaces requirement. Uninstalling HiRoute
SHALL first disable an enabled integration and SHALL refuse, with the reason, when
that is not possible.

#### Scenario: Enable on Omarchy

- **WHEN** a user with HiRoute installed enables the integration on Omarchy
- **THEN** the app launcher lists "Codex · Difficulty routing", "Claude Code · Difficulty routing" and "HiRoute", the top bar shows the HiRoute indicator, the native Codex and Claude Code entries are unchanged, and no native agent configuration file is modified.

#### Scenario: Not an Omarchy environment

- **WHEN** a user runs enable on a Linux system where Omarchy is not detected
- **THEN** the command fails with the missing Omarchy reason and writes no file.

#### Scenario: A changed entry survives disable

- **WHEN** the user edited the routed Codex launcher entry and then disables the integration
- **THEN** the edited entry is kept and reported as user-changed, every unchanged owned item is removed, and the Codex profile connection and published Plans are unchanged.

#### Scenario: Upgrade refreshes owned items

- **WHEN** HiRoute is upgraded while the integration is enabled and no owned item was changed
- **THEN** after the upgrade completes, the owned items match the installed release and the launcher shows each entry once.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/164#issuecomment-6262810004

### Requirement: Omarchy surfaces show routing status, notify once and open the console

When the integration is enabled, the top-bar indicator SHALL show today's headline
token share, the number of current trust suggestions and service health. For each
routed Agent it SHALL list the distinct Plans behind the model slots the Agent starts
with: Codex's default model, and Claude Code's Opus, Sonnet and Haiku presets. Each
Plan is listed once, labelled with the slots that use it. A slot mapped directly to a
fixed model is not a Plan and is not listed. Each listed Plan SHALL show one of three
trust presentations:

- its actual trust level, or Custom, and threshold, when its applicable difficulty
  trust controls share one level and threshold;
- "no trust level", when no difficulty trust control applies, such as a fixed-model,
  free-first or rule-based Plan;
- "configured per branch", when its branches use different settings.

The indicator SHALL NOT pick one branch to stand for the Plan or invent a level or
threshold. Missing or unreadable routing facts SHALL produce the unavailable state,
never "no trust level". It SHALL also show the number of other Plans the Agent can
switch to, with a way to see all of them in the console.

It SHALL refresh periodically and when opened, read only HiRoute's public CLI JSON,
and validate that output as untrusted data. When data is unavailable or invalid it
SHALL show an unavailable state rather than stale figures presented as current.
Selecting it SHALL open the console, directly or through one action in its panel.

The integration SHALL send a desktop notification once for each new trust suggestion.
It SHALL also notify when a routed-launch preflight fails, and when the service
becomes unavailable while the integration is enabled. Trust suggestions SHALL be
counted and notified across every Plan a routed Agent maps, not only the Plans shown
by slot. When the suggestions of some mapped Plans cannot be read, the indicator SHALL
disclose how many Plans are affected, rather than presenting them as having no
suggestions. A suggestion notification SHALL open that suggestion's evidence in the
console, and the user SHALL be able to turn notifications off.

The console entry SHALL obtain a new single-use login link on every open and open it
as an Omarchy web app. When no console is configured, enabling SHALL offer to
initialise a loopback-only console and SHALL NOT configure network listening. After
enabling, the first console open SHALL lead to the trial quick start, with Codex's
independent profile and Claude Code's independent mode as the offered targets, and
SHALL finish by pointing to the routed launcher entries. The indicator panel, the
integration status and the guide SHALL state that routed sessions use the subscription
connection configured in HiRoute and do not follow Omarchy's active account.
Indicator, notification and launcher text SHALL be available in Chinese and English
and follow the system locale.

#### Scenario: Indicator after a trial day

- **WHEN** the integration is enabled and a Codex trial Plan at Trial level handled routed turns today
- **THEN** the indicator shows today's share, Trial for Codex, zero or more pending suggestions and a healthy service, and the console is one selection away.

#### Scenario: A suggestion notifies once

- **WHEN** a trust step-up suggestion appears for the trial Plan and the indicator refreshes several times
- **THEN** exactly one notification is shown for that suggestion, and selecting it opens the suggestion's evidence in the console.

#### Scenario: Service unavailable

- **WHEN** the service stops while the integration is enabled
- **THEN** the indicator shows the unavailable state without stale figures and one notification reports the stopped service.

#### Scenario: Console without configuration

- **WHEN** the user enables the integration and no console is configured
- **THEN** enable offers to initialise a loopback-only console, and opening HiRoute afterwards uses a fresh single-use link in a web-app window.

#### Scenario: Claude maps several Plans

- **WHEN** Claude Code's independent mode maps its Opus preset to one Plan and its Sonnet and Haiku presets to another
- **THEN** the indicator panel lists both Plans with their trust presentations and the presets that use each, rather than choosing one Plan.

#### Scenario: Plans without one trust policy

- **WHEN** Claude Code's Sonnet preset maps a fixed-model Plan and its Haiku preset maps a Plan whose branches use different thresholds
- **THEN** the panel shows "no trust level" for the first Plan and "configured per branch" for the second, with no invented level or threshold.

#### Scenario: Codex can switch to more Plans

- **WHEN** Codex's independent profile starts on Plan A and also offers nine other Plans, one of which has a new trust suggestion
- **THEN** the panel shows Plan A's trust presentation and nine more Plans with a way to open the complete list in the console, and the suggestion for the other Plan is counted and notified once.

#### Scenario: Some suggestions cannot be read

- **WHEN** the suggestions of one mapped Plan cannot be read while another Plan has two
- **THEN** the panel shows two suggestions and that one Plan's suggestions are unavailable.

#### Scenario: First run

- **WHEN** the user opens HiRoute for the first time after enabling
- **THEN** the console leads to the trial quick start with Codex's independent profile and Claude Code's independent mode as targets and ends by pointing to the routed launcher entries.

Source SPEC comments:
- https://github.com/higress-group/HiRoute/issues/164#issuecomment-6262816032

## Implementation references

- [CLI family](../../../crates/cli/src/host_commands/omarchy.rs),
  [indicator summary](../../../crates/cli/src/host_commands/omarchy/summary.rs) and
  [host-owned files, notifications and launch boundary](../../../crates/host-runtime/src/desktop_integration/omarchy/README.md).
- [Bar widget and panel](../../../assets/integrations/omarchy/hiroute.status/Panel.qml)
  with its [strict summary reader](../../../assets/integrations/omarchy/hiroute.status/Summary.js).
- [Owned-file lifecycle scenarios](../../../crates/host-runtime/src/desktop_integration/omarchy/tests.rs),
  [summary scenarios](../../../crates/cli/src/host_commands/omarchy/summary_tests.rs),
  [widget reader scenarios](../../../crates/cli/src/host_commands/omarchy/summary_plugin.test.mjs)
  and [installer hooks](../../../scripts/test-standalone-install.py).
