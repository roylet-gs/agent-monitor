import { useState, useEffect, useCallback, useRef } from "react";
import { checkForUpdate, type UpdateCheckResult } from "../lib/version.js";
import type { Settings } from "../lib/types.js";
import { log } from "../lib/logger.js";

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
    if (!result) {
      log("debug", "version", "processResult received null (check failed or skipped)");
      return null;
    }
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

  // Periodic recheck every 10 minutes to catch CDN propagation delays
  useEffect(() => {
    const RECHECK_INTERVAL_MS = 10 * 60 * 1000;
    const timer = setInterval(() => {
      log("debug", "version", "periodic recheck triggered");
      checkForUpdate(settingsRef.current, { forceCheck: true }).then(processResult);
    }, RECHECK_INTERVAL_MS);
    return () => clearInterval(timer);
  }, [processResult]);

  const recheck = useCallback(async (): Promise<UpdateInfo | null> => {
    const result = await checkForUpdate(settingsRef.current, { forceCheck: true });
    return processResult(result);
  }, [processResult]);

  return { updateInfo, recheck };
}
