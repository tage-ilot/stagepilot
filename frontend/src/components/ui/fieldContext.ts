import { createContext, useContext } from "react";

/** Set by Field, read by Input/Select/Textarea. Outside a Field every value is empty, so controls stay usable alone. */
export interface FieldContextValue {
  id?: string;
  describedBy?: string;
  invalid?: boolean;
  held?: boolean;
}

export const FieldContext = createContext<FieldContextValue>({});
export const useFieldContext = () => useContext(FieldContext);

export const joinIds = (...ids: Array<string | undefined>) => ids.filter(Boolean).join(" ") || undefined;
