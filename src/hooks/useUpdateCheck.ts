import { useState, useEffect, useCallback, useRef } from "react";
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
): { updateInfo: UpdateInfo | null; recheck: () => Promise<UpdateInfo | null> } {
  const [updateInfo, setUpdateInfo] = useState<UpdateInfo | null>(null);
  const settingsRef = useRef(settings);
  const onSaveRef = useRef(onSaveSettings);

  useEffect(() => { settingsRef.current = settings; }, [settings]);
  useEffect(() => { onSaveRef.current = onSaveSettings; }, [onSaveSettings]);

  const processResult = useCallback((result: UpdateCheckResult | null): UpdateInfo | null => {
    if (!result) return null;
    const info: UpdateInfo = {
      current: result.current,
      latest: result.latest,
      updateAvailable: result.updateAvailable,
    };
    setUpdateInfo(info);
    // Persist cache fields
    const s = settingsRef.current;
    if (
      result.settings.lastUpdateCheck !== s.lastUpdateCheck ||
      result.settings.latestKnownVersion !== s.latestKnownVersion
    ) {
      onSaveRef.current({ ...s, ...result.settings });
    }
    return info;
  }, []);

  useEffect(() => {
    checkForUpdate(settings).then(processResult);
  }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const recheck = useCallback(async (): Promise<UpdateInfo | null> => {
    const result = await checkForUpdate(settingsRef.current, { forceCheck: true });
    return processResult(result);
  }, [processResult]);

  return { updateInfo, recheck };
}
