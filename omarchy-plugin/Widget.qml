import QtQuick
import Quickshell
import Quickshell.Io
import qs.Commons
import qs.Ui

// Facet bar widget: one icon with an optional live value, and a popup of headline readings with a
// way into the Facet app. Data comes from `facet --status` (JSON, ~0.1 s, never wakes a sleeping GPU).
// Nothing here changes hardware or writes files; it only reads and launches `facet-app`.
BarWidget {
  id: root
  moduleName: "facet.hardware"

  // ---- state -------------------------------------------------------------------------------
  property var readings: []
  property string machine: ""
  property bool installed: true
  property string errorText: ""
  property bool popupOpen: false

  readonly property string pillReading: String(setting("pillReading", "cpu_temperature"))
  readonly property int refreshMs: Math.max(5, Number(setting("refreshIntervalSec", 15))) * 1000
  readonly property var pill: {
    for (var i = 0; i < readings.length; i++)
      if (readings[i].id === pillReading) return readings[i]
    return null
  }
  // Nerd Font microchip glyph; the value follows when one is chosen and available.
  readonly property string pillText: "" + (pill && pill.available ? "  " + pill.display : "")

  // PATH for the shell process may not include ~/.local/bin (where a per-user install lands).
  readonly property string pathPrefix: "PATH=\"$HOME/.local/bin:/usr/local/bin:$PATH\"; "

  // ---- bar-panel contract (the bar routes open/close/toggle through these) -------------------
  readonly property bool opened: popupOpen
  readonly property bool popoutSwitchClosing: false
  function open() { popupOpen = true; poll() }
  function close() { popupOpen = false }
  function toggle() { popupOpen ? close() : open() }
  function closeForPopoutSwitch() { close() }
  function refresh() { poll() }

  function launchFacet() {
    close()
    Quickshell.execDetached(["sh", "-c", pathPrefix + "exec facet-app"])
  }

  function copyReport() {
    close()
    Quickshell.execDetached(["sh", "-c", pathPrefix + "facet | wl-copy && omarchy-notification-send 'Facet: hardware report copied to the clipboard'"])
  }

  function copyInstallCommand() {
    close()
    Quickshell.execDetached(["sh", "-c", "printf %s 'curl -fsSL https://raw.githubusercontent.com/last-refuge/omarchy-facet/master/install.sh | bash' | wl-copy && omarchy-notification-send 'Facet: install command copied. Paste it in a terminal.'"])
  }

  // ---- data ----------------------------------------------------------------------------------
  function poll() { if (!statusProc.running) statusProc.running = true }

  function parse(text) {
    try {
      var data = JSON.parse(text)
      root.machine = data.machine || ""
      root.readings = data.readings || []
      root.installed = true
      root.errorText = ""
    } catch (e) {
      root.errorText = "Could not read Facet's output."
    }
  }

  Process {
    id: statusProc
    command: ["sh", "-c", root.pathPrefix + "exec facet --status"]
    stdout: StdioCollector {
      waitForEnd: true
      onStreamFinished: root.parse(text)
    }
    onExited: function(code) {
      if (code === 127) { root.installed = false; root.readings = []; root.errorText = "" }
      else if (code !== 0) root.errorText = "facet --status failed (exit " + code + ")."
    }
  }

  Timer {
    interval: root.popupOpen ? 3000 : root.refreshMs
    repeat: true
    running: true
    triggeredOnStart: true
    onTriggered: root.poll()
  }

  IpcHandler {
    target: "facet.hardware"
    function open(): void { root.open() }
    function close(): void { root.close() }
    function toggle(): void { root.toggle() }
    function show(): void { root.open() }
    function hide(): void { root.close() }
    function launch(): void { root.launchFacet() }
  }

  // ---- bar button ----------------------------------------------------------------------------
  implicitWidth: button.implicitWidth
  implicitHeight: button.implicitHeight

  WidgetButton {
    id: button
    anchors.fill: parent
    bar: root.bar
    text: root.vertical ? "" : root.pillText
    active: root.popupOpen
    useActiveColor: false
    tooltipText: root.installed ? "Facet: hardware readings (right-click opens Facet)" : "Facet is not installed"
    onPressed: function(b) {
      if (b === Qt.RightButton) root.launchFacet()
      else if (b === Qt.MiddleButton) root.poll()
      else root.toggle()
    }
  }

  // ---- popup ---------------------------------------------------------------------------------
  PopupCard {
    id: popup
    anchorItem: button
    bar: root.bar
    owner: root
    open: root.popupOpen
    contentWidth: Style.space(300)
    contentHeight: content.implicitHeight + Style.spacing.popupPadding * 2

    Column {
      id: content
      width: parent.width
      spacing: Style.space(10)

      Column {
        width: parent.width
        spacing: Style.space(2)
        Text {
          text: "facet"
          color: Color.accent
          font.family: Style.font.family
          font.pixelSize: Style.font.heading
          font.bold: true
          renderType: Text.NativeRendering
        }
        Text {
          visible: text !== ""
          text: root.machine
          color: Color.muted
          font.family: Style.font.family
          font.pixelSize: Style.font.bodySmall
          elide: Text.ElideRight
          width: parent.width
          renderType: Text.NativeRendering
        }
      }

      Rectangle { width: parent.width; height: 1; color: Color.muted; opacity: 0.35 }

      // Not installed / failed
      Column {
        visible: !root.installed || root.errorText !== ""
        width: parent.width
        spacing: Style.space(8)
        Text {
          width: parent.width
          wrapMode: Text.WordWrap
          text: !root.installed ? "Facet isn't installed on this machine yet." : root.errorText
          color: Color.popups.text
          font.family: Style.font.family
          font.pixelSize: Style.font.body
          renderType: Text.NativeRendering
        }
      }

      // Readings
      Repeater {
        model: root.installed ? root.readings : []
        delegate: Item {
          width: content.width
          height: Math.max(labels.implicitHeight, valueText.implicitHeight)

          Column {
            id: labels
            anchors.left: parent.left
            anchors.right: valueText.left
            anchors.rightMargin: Style.space(10)
            spacing: Style.space(1)
            Text {
              text: modelData.label
              color: Color.popups.text
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              renderType: Text.NativeRendering
            }
            Text {
              text: modelData.note
              width: parent.width
              wrapMode: Text.WordWrap
              color: Color.muted
              font.family: Style.font.family
              font.pixelSize: Style.font.caption
              renderType: Text.NativeRendering
            }
          }

          Text {
            id: valueText
            anchors.right: parent.right
            anchors.top: parent.top
            text: modelData.display
            color: modelData.available ? Color.popups.text : Color.muted
            font.family: Style.font.family
            font.pixelSize: Style.font.title
            font.bold: modelData.available
            renderType: Text.NativeRendering
          }
        }
      }

      Rectangle { width: parent.width; height: 1; color: Color.muted; opacity: 0.35 }

      // Actions
      Row {
        width: parent.width
        spacing: Style.space(8)

        Repeater {
          model: root.installed
            ? [ { label: "Open Facet", primary: true, action: "launch" }, { label: "Copy report", primary: false, action: "report" } ]
            : [ { label: "Copy install command", primary: true, action: "install" } ]

          delegate: Rectangle {
            width: (content.width - Style.space(8) * (root.installed ? 1 : 0)) / (root.installed ? 2 : 1)
            height: Style.space(30)
            radius: Style.cornerRadius
            color: modelData.primary ? Color.accent : "transparent"
            border.width: modelData.primary ? 0 : 1
            border.color: Color.muted
            opacity: mouse.containsMouse ? 0.85 : 1

            Text {
              anchors.centerIn: parent
              text: modelData.label
              color: modelData.primary ? Color.background : Color.popups.text
              font.family: Style.font.family
              font.pixelSize: Style.font.body
              font.bold: modelData.primary
              renderType: Text.NativeRendering
            }

            MouseArea {
              id: mouse
              anchors.fill: parent
              hoverEnabled: true
              cursorShape: Qt.PointingHandCursor
              onClicked: {
                if (modelData.action === "launch") root.launchFacet()
                else if (modelData.action === "report") root.copyReport()
                else root.copyInstallCommand()
              }
            }
          }
        }
      }
    }
  }
}
