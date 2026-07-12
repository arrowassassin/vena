// Local shims for the design runtime — loaded BEFORE support.js.
// The x-import loader wants Babel (from a CDN) for .jsx modules; this sandbox
// is offline, and our local ./ios-frame.jsx is written in plain JS
// (React.createElement, no JSX syntax), so an identity "transform" suffices.
(function () {
  if (!window.Babel) {
    window.Babel = { transform: function (src) { return { code: src }; } };
  }
})();
