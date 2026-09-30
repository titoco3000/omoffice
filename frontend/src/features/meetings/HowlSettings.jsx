//! Howl guard rows for the profile page's settings panel, styled like
//! `ThemeToggle`.

import OnOffToggle from "../../core/ui/OnOffToggle";
import { setHowlSettings, useHowlSettings } from "./howlSettings";

/// The howl guard's rows, laid out by the caller's `Row` so they line up
/// with the rest of the settings panel.
export default function HowlSettings({ Row }) {
  const { enabled } = useHowlSettings();
  return (
    <>
      <Row label="Mute mic on feedback">
        <OnOffToggle
          value={enabled}
          onChange={(next) => setHowlSettings({ enabled: next })}
        />
      </Row>
    </>
  );
}
