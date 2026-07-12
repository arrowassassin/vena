// image-slot — drop-target placeholder used by the design for character art.
// Real portraits arrive via generate_portrait/image:done; until wired, this
// renders an honest empty frame with the placeholder hint.
window["image-slot"] = function ImageSlot(props) {
  var h = React.createElement;
  var src = props.src || (window.__venaSlotArt && window.__venaSlotArt[props.id]);
  if (src) {
    return h("img", {
      src: src,
      alt: props.placeholder || "",
      style: { width: "100%", height: "100%", objectFit: props.fit || "cover", display: "block" },
    });
  }
  return h(
    "div",
    {
      title: props.placeholder || "",
      style: {
        width: "100%",
        height: "100%",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        border: "1px dashed rgba(246,243,236,.28)",
        color: "rgba(246,243,236,.4)",
        fontFamily: "'IBM Plex Mono',monospace",
        fontSize: "6px",
        letterSpacing: ".08em",
        textAlign: "center",
        padding: "4px",
        overflow: "hidden",
      },
    },
    ""
  );
};
