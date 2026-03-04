import { useState, useEffect } from "react";
import { checkForUpdate, type UpdateCheckResult } from "../lib/version.js";
import type { Settings } from "../lib/types.js";

export interface UpdateInfo {
  current: string;
  latest: string;
  updateAvailable: boolean;
}

export function useUpdateCheck(
  settings: Settings,
  onSaveSettings: (settings: Settings) => void
): UpdateInfo | null {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    checkForUpdate(settings).then((result: UpdateCheckResult | null) => {
      if (!result) return;
      setUpdateInfo({
        current: result.current,
        latest: result.latest,
        updateAvailable: result.updateAvailable,
      });
      // Persist cache fields
      if (
        result.settings.lastUpdateCheck !== settings.lastUpdateCheck ||
        result.settings.latestKnownVersion !== settings.latestKnownVersion
      ) {
        onSaveSettings({ ...settings, ...result.settings });
      }
    });
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  return updateInfo;
}
