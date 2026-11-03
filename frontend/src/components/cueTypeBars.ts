import { CUE_TYPES, type CueTypeId } from "./lightingCueTypes";

/** The bar piece: solid, hollow, dashed or striped, in the type colour. */
export const barClassFor = (type: CueTypeId, sharp = false): string => `${CUE_TYPES[type].bar}${sharp ? " cue-bar-sharp" : ""}`;
