import { useState } from "react";
import {
  IconCamera,
  IconCameraOff,
  IconClose,
  IconLeave,
  IconMic,
  IconMicOff,
  IconScreen,
  IconScreenOff,
  IconSettings,
} from "../ui/icons";

/// Where the control panel may sit. Keys are the setting's stored value.
export const PANEL_POSITIONS = {
  "top left": "top-3 left-3",
  "top center": "top-3 left-1/2 -translate-x-1/2",
  "top right": "top-3 right-3",
  "bottom left": "bottom-3 left-3",
  "bottom center": "bottom-3 left-1/2 -translate-x-1/2",
  "bottom right": "bottom-3 right-3",
};

/// Where the other participants go while one is expanded.
export const PARTICIPANT_POSITIONS = ["top", "bottom", "left", "right"];

/**
 * The overlay panel: settings, mic, camera, screen share, leave. The
 * settings popover opens away from whichever edge it's pinned to.
 */
export default function ControlPanel({
  settings,
  onSettings,
  muted,
  onToggleMute,
  cameraOn,
  onToggleCamera,
  sharing,
  onToggleShare,
  onLeave,
}) {
  const [open, setOpen] = useState(false);
  const onTop = settings.panel.startsWith("top");
  const onRight = settings.panel.endsWith("right");

  return (
    <div
      className={`absolute z-10 ${PANEL_POSITIONS[settings.panel]}`}
      // A click inside the panel must not reach the tile underneath it.
      onClick={(event) => event.stopPropagation()}
    >
      <div className="flex items-center gap-1 rounded-card border border-line bg-surface/85 p-1.5 shadow-lg backdrop-blur">
        <PanelButton
          icon={<IconSettings />}
          label="settings"
          active={open}
          onClick={() => setOpen((current) => !current)}
        />
        <PanelButton
          icon={muted ? <IconMicOff /> : <IconMic />}
          label={muted ? "unmute" : "mute"}
          danger={muted}
          onClick={onToggleMute}
        />
        <PanelButton
          icon={cameraOn ? <IconCamera /> : <IconCameraOff />}
          label={cameraOn ? "turn camera off" : "turn camera on"}
          danger={!cameraOn}
          onClick={onToggleCamera}
        />
        <PanelButton
          icon={sharing ? <IconScreenOff /> : <IconScreen />}
          label={sharing ? "stop sharing" : "share screen"}
          active={sharing}
          onClick={onToggleShare}
        />
        <span className="mx-0.5 h-6 w-px bg-line" />
        <PanelButton
          icon={<IconLeave />}
          label="leave"
          danger
          onClick={onLeave}
        />
      </div>

      {open && (
        <div
          className={`absolute w-60 rounded-card border border-line bg-surface/95 p-3 shadow-lg backdrop-blur ${
            onTop ? "top-full mt-2" : "bottom-full mb-2"
          } ${onRight ? "right-0" : "left-0"}`}
        >
          <div className="flex items-center justify-between">
            <h3 className="text-xs font-semibold text-ink">settings</h3>
            <button
              type="button"
              onClick={() => setOpen(false)}
              aria-label="close settings"
              className="text-ink-muted hover:text-ink"
            >
              <IconClose className="h-4 w-4" />
            </button>
          </div>

          <Choice
            title="panel position"
            options={Object.keys(PANEL_POSITIONS)}
            value={settings.panel}
            onChange={(panel) => onSettings({ ...settings, panel })}
            columns="grid-cols-3"
          />
          <Choice
            title="participants"
            options={PARTICIPANT_POSITIONS}
            value={settings.participants}
            onChange={(participants) =>
              onSettings({ ...settings, participants })
            }
            columns="grid-cols-4"
          />
          <p className="mt-2 text-[11px] leading-snug text-ink-faint">
            participants sit there while one of them is expanded.
          </p>
        </div>
      )}
    </div>
  );
}

function Choice({ title, options, value, onChange, columns }) {
  return (
    <div className="mt-3">
      <p className="font-mono text-[11px] text-ink-faint">{title}</p>
      <div className={`mt-1.5 grid gap-1 ${columns}`}>
        {options.map((option) => (
          <button
            key={option}
            type="button"
            onClick={() => onChange(option)}
            className={`rounded-md px-1.5 py-1 text-[11px] leading-tight transition-colors ${
              option === value
                ? "bg-brand text-on-brand"
                : "bg-muted text-ink-muted hover:bg-line"
            }`}
          >
            {option}
          </button>
        ))}
      </div>
    </div>
  );
}

function PanelButton({ icon, label, onClick, active = false, danger = false }) {
  const tone = danger
    ? "bg-danger text-on-danger hover:opacity-90"
    : active
      ? "bg-brand text-on-brand hover:bg-brand-strong"
      : "text-ink hover:bg-muted";
  return (
    <button
      type="button"
      onClick={onClick}
      title={label}
      aria-label={label}
      className={`rounded-lg p-2 transition-colors ${tone}`}
    >
      {icon}
    </button>
  );
}
