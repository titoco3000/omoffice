function ControlButton({ active, danger, children, ...props }) {
  return (
    <button
      {...props}
      className={`rounded-card border px-3 py-1.5 text-sm disabled:opacity-50 ${
        danger
          ? "text-danger hover:bg-danger/10"
          : active
            ? "text-ink"
            : "text-ink-muted hover:text-ink"
      }`}
    >
      {children}
    </button>
  );
}

export default function MeetingControls({
  cameraOn,
  micMuted,
  sharingScreen,
  canShareScreen,
  onLeave,
  onToggleCamera,
  onToggleMic,
  onToggleScreenShare,
}) {
  return (
    <div className="flex flex-wrap gap-2">
      <ControlButton danger onClick={onLeave}>
        Leave
      </ControlButton>
      <ControlButton active={cameraOn} onClick={onToggleCamera}>
        {cameraOn ? "Turn camera off" : "Turn camera on"}
      </ControlButton>
      <ControlButton active={!micMuted} onClick={onToggleMic}>
        {micMuted ? "Unmute" : "Mute"}
      </ControlButton>
      <ControlButton
        active={sharingScreen}
        disabled={!canShareScreen}
        onClick={onToggleScreenShare}
      >
        {sharingScreen ? "Stop sharing screen" : "Share screen"}
      </ControlButton>
    </div>
  );
}
