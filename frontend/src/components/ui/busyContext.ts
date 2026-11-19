import { createContext, useContext } from "react";

/** Set by `SettingsSection` while its request runs: every Input and Select inside is disabled without each page passing `disabled`. */
export const BusyContext = createContext(false);
export const useBusy = () => useContext(BusyContext);
