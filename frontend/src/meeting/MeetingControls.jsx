import {
  FiLock,
  FiMic,
  FiMicOff,
  FiMonitor,
  FiPhoneOff,
  FiStopCircle,
  FiUnlock,
  FiVideo,
  FiVideoOff,
} from "react-icons/fi";
import AsyncButton from "../components/AsyncButton";

function ControlButton({ active, danger, icon, children, ...props }) {
  return (
    <AsyncButton
      {...props}
      className={`flex items-center gap-1.5 rounded-card border px-3 py-1.5 text-sm disabled:opacity-50 ${
        danger
          ? "text-danger hover:bg-danger/10"
          : active
            ? "text-ink"
            : "text-ink-muted hover:text-ink"
      }`}
    >
      {icon}
      {children}
    </AsyncButton>
  );
}

export default function MeetingControls({
  cameraOn,
  micMuted,
  sharingScreen,
  locked,
  canShareScreen,
  onLeave,
  onToggleCamera,
  onToggleMic,
  onToggleScreenShare,
  onToggleLock,
}) {
  return (
    <div className="flex flex-wrap gap-2">
      <ControlButton danger icon={<FiPhoneOff />} onClick={onLeave}>
        Leave
      </ControlButton>
      <ControlButton
        active={locked}
        icon={locked ? <FiLock /> : <FiUnlock />}
        onClick={onToggleLock}
        title={locked ? "Unlock meeting" : "Lock meeting"}
      ></ControlButton>
      <ControlButton
        active={cameraOn}
        icon={cameraOn ? <FiVideoOff /> : <FiVideo />}
        onClick={onToggleCamera}
        title={cameraOn ? "Turn camera off" : "Turn camera on"}
      ></ControlButton>
      <ControlButton
        active={!micMuted}
        icon={micMuted ? <FiMic /> : <FiMicOff />}
        onClick={onToggleMic}
        title={micMuted ? "Unmute" : "Mute"}
      ></ControlButton>
      <ControlButton
        active={sharingScreen}
        disabled={!canShareScreen}
        icon={sharingScreen ? <FiStopCircle /> : <FiMonitor />}
        onClick={onToggleScreenShare}
      >
        {sharingScreen ? "Stop sharing screen" : "Share screen"}
      </ControlButton>
    </div>
  );
}
