# Use HiRoute on Omarchy

The optional Omarchy integration lets Codex and Claude Code sessions started from the Omarchy desktop go through HiRoute. Turns that need your subscription model keep using it, and the rest go to the other models in your Plan, so the subscription lasts longer. Omarchy's own agent entries stay unchanged; HiRoute adds its own next to them.

Enabling it adds:

- two app launcher entries, **Codex · Difficulty routing** and **Claude Code · Difficulty routing**, plus a **HiRoute** entry that opens the console;
- a HiRoute widget on the right of the Omarchy bar, with a panel;
- desktop notifications for trust suggestions, an unavailable service and failed routed launches.

## 1. Before you start

- Install HiRoute headless and start the service as described in [Install HiRoute headless on Linux](/en/docs/install-linux/). The integration needs that current-user installation.
- Install Codex and/or Claude Code as you normally would.
- Run the commands below in a terminal inside your Omarchy session. HiRoute detects Omarchy from `OMARCHY_PATH` and the `omarchy` command. Elsewhere, enable stops without writing anything.

## 2. Enable the integration

```sh
hiroute integrations omarchy enable --console yes
```

Enable writes the launcher entries, the HiRoute icon and the `hiroute.status` bar widget for the current user. It then adds the widget to the bar through Omarchy's own `omarchy plugin` commands. It never edits Omarchy's files and never starts, stops or restarts HiRoute. If a file it would write already exists and HiRoute does not own it, enable stops and names it instead of taking it over.

`--console yes` configures a loopback-only console when none is configured yet; `--console no` skips it. If enable configured the console just now, restart the service once so that it listens:

```sh
hiroute service restart --output json
```

Notifications start on. Add `--notifications off` to start without them.

Check the result at any time:

```sh
hiroute integrations omarchy status
```

It reports each HiRoute-owned file as `present`, `missing`, `changed` or `outdated`, the launch check for each routed agent, whether the console is configured and listening, and the notifications preference.

## 3. Choose a model to try

Open **HiRoute** from the app launcher, or **Open HiRoute** from the bar panel. The first time after enabling, the console opens the trial quick start with Codex and Claude Code already selected as targets:

- Codex uses an independent HiRoute profile.
- Claude Code uses its independent mode.

Both affect only sessions started through HiRoute. Running `codex` or `claude` directly, and their native settings, stay as they were. Choose the model to try and finish the quick start; its last step points you to the two launcher entries.

## 4. Start a routed session

Pick **Codex · Difficulty routing** or **Claude Code · Difficulty routing** in the app launcher. Each entry starts the agent with Omarchy's default interactive arguments, `--approve-for-me` for Codex and `--permission-mode auto` for Claude Code, through:

```sh
hiroute integrations omarchy launch --agent codex -- --approve-for-me
```

The session behaves like Omarchy's own agent entries:

- It opens in a terminal with the same window class, so your window rules still apply.
- When started from your home directory and `~/Work` exists, it starts in `~/Work`; otherwise it keeps the current directory.

Each routed session uses an independent HiRoute connection. In a terminal, `hiroute agent launch --agent codex -- <args>` starts the same routed session without the desktop conventions.

> **Account boundary.** Routed sessions use the subscription connection configured in HiRoute. They do not follow Omarchy's active account. Switching accounts in Omarchy does not change the account a routed session uses.

## 5. Read the bar indicator

The widget shows the share of today's tokens served outside your subscription, for example `42.5%`. The figure is approximate across models; it is not a quota, a cost or a saving. Its tooltip adds "Some usage is unknown" when some requests recorded no usage. When there is no share to show, the widget shows a dash and the tooltip says why: no routed requests today, no subscription source used, subscription usage unknown, or no comparable usage.

A dot marks pending trust suggestions. The widget turns to the warning colour when HiRoute is unavailable or a routed agent cannot launch.

Click the widget to open its panel:

- For each routed agent, the Plans behind its start slots: Codex's default model, and Claude Code's Opus, Sonnet and Haiku. Each Plan shows its trust level and threshold. A Plan without difficulty trust controls shows "No trust level", and one whose branches use different settings shows "Configured per branch". **N more Plans** opens that agent's complete Plan list in HiRoute.
- The number of trust suggestions, and how many Plans' suggestions could not be read.
- The account boundary above.
- **Open HiRoute**, **Routing timeline**, which follows the latest routed session in a terminal with `hiroute sessions timeline <SESSION_ID> --follow`, and **Turn notifications on/off**.

Right-click the widget to open HiRoute directly. The widget refreshes every minute and whenever you open the panel. If it cannot read current figures in time, it shows "HiRoute is unavailable" instead of older figures.

The official installer refreshes the widget files when it upgrades HiRoute. A running Omarchy shell can keep the previous widget loaded until it restarts. Restart it with `omarchy restart shell`, or log out and in again.

## 6. Notifications

HiRoute sends a desktop notification:

- once for each new trust suggestion; selecting it opens the suggestion's evidence in HiRoute;
- once each time the service goes from ready to unavailable;
- when a routed launch fails its readiness check.

Turn them off from the panel or with:

```sh
hiroute integrations omarchy configure --notifications off
```

Turning notifications off also drops notifications that were waiting to be sent.

## 7. When a routed launch cannot start

Each routed entry checks readiness before starting the agent. If the check fails, for example because the service is stopped, an agent upgraded by mise needs reconnecting, or a mapped Plan is no longer published, you get one notification. The terminal then explains the reason and offers to start the agent natively.

The native start happens only after you type `y`. It is not routed, uses your normal environment, and does not apply Omarchy's per-account selection. Nothing is ever started unrouted without your confirmation.

## 8. Disable and uninstall

```sh
hiroute integrations omarchy disable
```

Disable removes the widget from the bar through Omarchy, deletes the HiRoute-owned files you have not changed, and keeps and reports the ones you edited. Enable it again at any time.

The [standalone uninstaller](/en/docs/install-linux/) runs disable first. If disable fails, uninstall stops and reports why, so no integration file is left without an owner.
