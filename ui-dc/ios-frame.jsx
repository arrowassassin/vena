// IOSDevice — minimal phone shell for the mobile design (plain JS, no JSX:
// the runtime's Babel shim is an identity transform). Renders a status-bar
// overlay (the template's app bar pads 58px to clear it) and the app content.
window.IOSDevice = function IOSDevice(props) {
  var h = React.createElement;
  var dark = !!props.dark;
  return h(
    "div",
    {
      style: {
        width: "min(430px, 100vw)",
        minHeight: "100vh",
        margin: "0 auto",
        position: "relative",
        background: dark ? "#0c0c10" : "#111",
        overflow: "hidden",
        display: "flex",
        flexDirection: "column",
      },
    },
    // iOS status bar overlay
    h(
      "div",
      {
        style: {
          position: "absolute",
          top: 0, left: 0, right: 0,
          height: "44px",
          zIndex: 40,
          display: "flex",
          alignItems: "center",
          justifyContent: "space-between",
          padding: "10px 24px 0",
          pointerEvents: "none",
          fontFamily: "'IBM Plex Mono',monospace",
          fontSize: "12px",
          fontWeight: 600,
          color: dark ? "#f6f3ec" : "#15151a",
          mixBlendMode: dark ? "normal" : "multiply",
        },
      },
      h("span", null, "9:41"),
      h("span", { style: { letterSpacing: ".08em" } }, "▪▪▪ 🔋")
    ),
    props.children
  );
};
