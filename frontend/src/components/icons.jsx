/**
 * The handful of icons the meeting UI needs, inline rather than a package —
 * the Chime SDK already dominates the bundle.
 */

function Svg({ className = "h-5 w-5", children }) {
  return (
    <svg
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.8"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

/// A line through an icon, for every "off" variant.
function Slash() {
  return <path d="M3 3l18 18" />;
}

export function IconSettings(props) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1A1.6 1.6 0 0 0 9 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1A1.6 1.6 0 0 0 4.6 9a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1A1.6 1.6 0 0 0 9 4.6a1.6 1.6 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z" />
    </Svg>
  );
}

export function IconMic(props) {
  return (
    <Svg {...props}>
      <rect x="9" y="2.5" width="6" height="11" rx="3" />
      <path d="M5.5 11a6.5 6.5 0 0 0 13 0M12 17.5V21M8.5 21h7" />
    </Svg>
  );
}

export function IconMicOff(props) {
  return (
    <Svg {...props}>
      <path d="M9 5a3 3 0 0 1 6 0v5m-6 .5V10" />
      <path d="M5.5 11a6.5 6.5 0 0 0 10.4 5.2M18.5 11v.5M12 17.5V21M8.5 21h7" />
      <Slash />
    </Svg>
  );
}

export function IconCamera(props) {
  return (
    <Svg {...props}>
      <rect x="2.5" y="6" width="13" height="12" rx="2.5" />
      <path d="M15.5 10.5l6-3.5v10l-6-3.5z" />
    </Svg>
  );
}

export function IconCameraOff(props) {
  return (
    <Svg {...props}>
      <path d="M15.5 11.5V8.5A2.5 2.5 0 0 0 13 6H8.5M2.6 6.6A2.5 2.5 0 0 0 2.5 8.5v7A2.5 2.5 0 0 0 5 18h8a2.5 2.5 0 0 0 2.4-1.8" />
      <path d="M21.5 7l-6 3.5v3l6 3.5z" />
      <Slash />
    </Svg>
  );
}

export function IconScreen(props) {
  return (
    <Svg {...props}>
      <rect x="2.5" y="4" width="19" height="12.5" rx="2" />
      <path d="M9 20.5h6M12 16.5v4" />
      <path d="M12 12.5v-5M9.5 10l2.5-2.5L14.5 10" />
    </Svg>
  );
}

export function IconScreenOff(props) {
  return (
    <Svg {...props}>
      <path d="M8 4h11.5a2 2 0 0 1 2 2v8.5a2 2 0 0 1-1 1.7M17 16.5H4.5a2 2 0 0 1-2-2V6a2 2 0 0 1 1.2-1.8" />
      <path d="M9 20.5h6M12 16.5v4" />
      <Slash />
    </Svg>
  );
}

export function IconLeave(props) {
  return (
    <Svg {...props}>
      <path d="M15 17l5-5-5-5M20 12H9M12 3.5H6.5a2 2 0 0 0-2 2v13a2 2 0 0 0 2 2H12" />
    </Svg>
  );
}

/// Stands in for a participant whose camera is off.
export function IconUser(props) {
  return (
    <Svg {...props}>
      <circle cx="12" cy="8.5" r="3.75" />
      <path d="M4.5 20.5a7.5 7.5 0 0 1 15 0" />
    </Svg>
  );
}

export function IconExpand(props) {
  return (
    <Svg {...props}>
      <path d="M4 9.5V4h5.5M20 14.5V20h-5.5M4 4l6 6M20 20l-6-6" />
    </Svg>
  );
}

export function IconCollapse(props) {
  return (
    <Svg {...props}>
      <path d="M9.5 4v5.5H4M14.5 20v-5.5H20M4 9.5L9.5 4M20 14.5L14.5 20" />
    </Svg>
  );
}

export function IconClose(props) {
  return (
    <Svg {...props}>
      <path d="M6 6l12 12M18 6L6 18" />
    </Svg>
  );
}
