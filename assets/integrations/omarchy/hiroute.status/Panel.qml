import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Commons as Commons
import qs.Ui
import "Summary.js" as Summary

// HiRoute indicator. It reads only `hiroute integrations omarchy summary`, runs every
// command as an argv array, and shows nothing as current unless the latest read validated.
Panel {
  id: root
  moduleName: "hiroute.status"
  ipcTarget: "hiroute.status"

  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  readonly property string cli: Quickshell.env("HOME") + "/.local/bin/hiroute"
  readonly property color foreground: bar ? bar.foreground : Commons.Color.foreground
  readonly property color dim: Qt.darker(foreground, 1.55)
  readonly property string fontFamily: bar ? bar.fontFamily : Style.font.family

  // The last validated summary, or null after any failed, late, oversized or invalid read.
  property var summary: null
  readonly property var barState: Summary.bar(summary)
  readonly property bool ready: summary !== null && summary.service.state === "ready"
  property string output: ""
  property bool rejected: false

  Commons.Translations { id: strings; directory: Qt.resolvedUrl("i18n") }

  function refresh() {
    if (summaryProcess.running) return
    output = ""
    rejected = false
    summaryProcess.running = true
    deadline.restart()
  }

  function finish(exitCode) {
    deadline.stop()
    summary = !rejected && exitCode === 0 ? Summary.parse(output) : null
    output = ""
    // Due notifications are decided by the CLI after every refresh, whatever was shown.
    if (!notifyProcess.running) notifyProcess.running = true
  }

  function run(args) {
    Quickshell.execDetached([root.cli, "integrations", "omarchy"].concat(args))
  }

  function openConsole(landing) {
    root.close()
    root.run(landing ? ["open-console", "--landing", landing] : ["open-console"])
  }

  function openTimeline() {
    var session = summary && summary.latest_routed_session
    if (!session) return
    root.close()
    Quickshell.execDetached(["omarchy-launch-tui", "--app-id=org.hiroute.timeline", root.cli,
                             "sessions", "timeline", session.session_id, "--follow"])
  }

  function number(value, decimals) {
    return Commons.I18n.formatNumber(value, decimals || 0)
  }

  function headlineText() {
    var state = root.barState
    if (!root.ready) return strings.t("panel.unavailable")
    if (state.percent >= 0) return strings.t("bar.share").arg(root.number(state.percent, state.decimals))
    return strings.t(state.tooltipKey)
  }

  function barTooltip() {
    if (!root.ready) return strings.t("bar.unavailable")
    var lines = [root.headlineText()]
    if (root.barState.percent >= 0 && root.barState.unknown) lines.push(strings.t("bar.someUnknown"))
    if (root.barState.agentWarning) lines.push(strings.t("bar.agentWarning"))
    if (root.barState.dot)
      lines.push(strings.tn("suggestions.count", summary.suggestions.count).arg(root.number(summary.suggestions.count)))
    return lines.join("\n")
  }

  function reasonText() {
    var code = summary ? summary.service.reason_code : ""
    if (code === "service_unavailable" || code === "summary_read_unavailable") return strings.t("reason." + code)
    return strings.t("reason.other")
  }

  function trustText(plan) {
    var trust = Summary.trust(plan)
    if (trust.threshold < 0) return strings.t(trust.key)
    return strings.t("trust.withThreshold").arg(strings.t(trust.key)).arg(root.number(trust.threshold / 1000, 2))
  }

  function slotsText(plan) {
    var keys = Summary.slotKeys(plan)
    var names = []
    for (var i = 0; i < keys.length; i++) names.push(strings.t(keys[i]))
    return names.join(", ")
  }

  onOpenedChanged: if (opened) refresh()

  Process {
    id: summaryProcess
    command: [root.cli, "integrations", "omarchy", "summary"]
    stdout: SplitParser {
      splitMarker: ""
      onRead: function(data) {
        if (root.rejected) return
        root.output += data
        if (root.output.length > Summary.MAX_BYTES) {
          root.rejected = true
          root.output = ""
          summaryProcess.running = false
        }
      }
    }
    onExited: function(exitCode, exitStatus) { root.finish(exitCode) }
  }

  Timer {
    id: deadline
    interval: 5000
    onTriggered: {
      root.rejected = true
      root.summary = null
      summaryProcess.running = false
    }
  }

  Process {
    id: notifyProcess
    command: [root.cli, "integrations", "omarchy", "notify"]
  }

  Process {
    id: configureProcess
    onExited: root.refresh()
  }

  Timer {
    interval: 60000
    running: true
    repeat: true
    triggeredOnStart: true
    onTriggered: root.refresh()
  }

  BarIconButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: "󰑪"
    labelText: root.barState.percent >= 0 ? root.number(root.barState.percent, root.barState.decimals) + "%"
      : (root.ready ? "–" : "")
    active: !root.ready || root.barState.warning
    tooltipText: root.barTooltip()
    onPressed: function(buttonCode) {
      if (buttonCode === Qt.RightButton) root.openConsole("")
      else root.toggle()
    }
  }

  Rectangle {
    visible: root.ready && root.barState.dot
    width: Math.max(4, Style.space(5))
    height: width
    radius: width / 2
    color: Commons.Color.accent
    anchors.top: button.top
    anchors.right: button.right
    anchors.topMargin: Style.space(4)
    anchors.rightMargin: Style.space(4)
  }

  component PlainLine: Text {
    textFormat: Text.PlainText
    color: root.foreground
    font.family: root.fontFamily
    font.pixelSize: Style.font.body
    wrapMode: Text.Wrap
  }

  component Action: Rectangle {
    id: action
    property string label: ""
    property bool available: true
    signal activated()
    implicitWidth: actionText.implicitWidth + Style.spacing.controlPaddingX * 2
    implicitHeight: Math.max(Style.spacing.controlHeight, actionText.implicitHeight + Style.spacing.controlPaddingY * 2)
    radius: Style.cornerRadius
    color: mouse.containsMouse && available ? Style.hoverFillFor(root.foreground, Commons.Color.accent)
      : Style.normalFillFor(root.foreground, Commons.Color.accent)
    Text {
      id: actionText
      anchors.centerIn: parent
      textFormat: Text.PlainText
      text: action.label
      color: action.available ? root.foreground : root.dim
      font.family: root.fontFamily
      font.pixelSize: Style.font.body
    }
    MouseArea {
      id: mouse
      anchors.fill: parent
      hoverEnabled: true
      enabled: action.available
      cursorShape: action.available ? Qt.PointingHandCursor : Qt.ArrowCursor
      onClicked: action.activated()
    }
  }

  KeyboardPanel {
    id: panel
    anchorItem: button
    owner: root
    bar: root.bar
    open: root.opened
    focusTarget: panel.loadedContent
    contentWidth: panel.fittedContentWidth(Style.space(380))
    contentHeight: panel.fittedContentHeight(panel.loadedContentHeight, Style.space(760))

    content: PanelKeyCatcher {
      id: keyCatcher
      anchors.fill: parent
      implicitHeight: column.implicitHeight
      onCloseRequested: root.close()
      onTextKey: function(text, modifiers) { if (text === "r") root.refresh() }

      Flickable {
        anchors.fill: parent
        contentHeight: column.implicitHeight
        clip: true
        boundsBehavior: Flickable.StopAtBounds

        Column {
          id: column
          width: parent.width
          spacing: Style.space(12)

          PanelHero {
            title: "HiRoute"
            meta: root.ready ? strings.t("headline.share") : strings.t("panel.unavailable")
            detail: root.ready && root.barState.percent >= 0 ? root.number(root.barState.percent, root.barState.decimals) + "%" : ""
            foreground: root.foreground
            fontFamily: root.fontFamily
          }

          PlainLine {
            width: column.width
            visible: root.ready
            text: root.ready ? root.headlineText() + (root.barState.percent >= 0 && root.barState.unknown ? " · " + strings.t("bar.someUnknown") : "") : ""
          }

          PlainLine {
            width: column.width
            visible: !root.ready
            color: root.dim
            text: root.reasonText()
          }

          Repeater {
            model: root.ready ? root.summary.agents : []

            Column {
              id: agentBlock
              required property var modelData
              width: column.width
              spacing: Style.space(4)

              PanelSeparator { width: parent.width; foreground: root.foreground }

              PlainLine {
                width: parent.width
                font.bold: true
                text: strings.t("agent." + agentBlock.modelData.agent)
              }

              PlainLine {
                width: parent.width
                color: agentBlock.modelData.launchable ? root.dim : (root.bar ? root.bar.urgent : Commons.Color.urgent)
                text: agentBlock.modelData.launchable ? strings.t("agent.ready")
                  : strings.t("agent.notLaunchable").arg(agentBlock.modelData.check_reason_code)
              }

              Repeater {
                model: agentBlock.modelData.slot_plans

                Column {
                  id: planBlock
                  required property var modelData
                  width: agentBlock.width
                  spacing: Style.space(1)

                  PlainLine {
                    width: parent.width
                    wrapMode: Text.NoWrap
                    elide: Text.ElideRight
                    text: planBlock.modelData.plan_name
                  }

                  PlainLine {
                    width: parent.width
                    color: root.dim
                    font.pixelSize: Style.font.bodySmall
                    text: root.slotsText(planBlock.modelData) + " · " + root.trustText(planBlock.modelData)
                  }
                }
              }

              PlainLine {
                width: parent.width
                visible: agentBlock.modelData.slot_plans.length === 0
                color: root.dim
                text: strings.t("agent.noSlotPlans")
              }

              Action {
                visible: agentBlock.modelData.other_mapped_plans > 0
                label: strings.tn("agent.morePlans", agentBlock.modelData.other_mapped_plans)
                  .arg(root.number(agentBlock.modelData.other_mapped_plans))
                onActivated: root.openConsole("agent:" + agentBlock.modelData.agent)
              }
            }
          }

          PanelSeparator { width: column.width; foreground: root.foreground; visible: root.ready }

          PlainLine {
            width: column.width
            visible: root.ready
            text: !root.ready ? "" : root.summary.suggestions.count > 0
              ? strings.tn("suggestions.count", root.summary.suggestions.count).arg(root.number(root.summary.suggestions.count))
              : strings.t("suggestions.none")
          }

          PlainLine {
            width: column.width
            visible: root.ready && root.summary.suggestions.unavailable_plans > 0
            color: root.dim
            text: root.ready ? strings.tn("suggestions.unavailable", root.summary.suggestions.unavailable_plans)
              .arg(root.number(root.summary.suggestions.unavailable_plans)) : ""
          }

          PlainLine {
            width: column.width
            color: root.dim
            font.pixelSize: Style.font.bodySmall
            text: strings.t("account.boundary")
          }

          Flow {
            width: column.width
            spacing: Style.space(6)

            Action {
              label: strings.t("action.open")
              onActivated: root.openConsole("")
            }

            Action {
              available: root.ready && root.summary.latest_routed_session !== null
              label: available ? strings.t("action.timeline") : strings.t("action.noSession")
              onActivated: root.openTimeline()
            }

            Action {
              visible: root.summary !== null
              label: root.summary && root.summary.notifications ? strings.t("action.notificationsOff") : strings.t("action.notificationsOn")
              onActivated: {
                if (configureProcess.running) return
                configureProcess.command = [root.cli, "integrations", "omarchy", "configure", "--notifications",
                                            root.summary.notifications ? "off" : "on"]
                configureProcess.running = true
              }
            }
          }
        }
      }
    }
  }
}
