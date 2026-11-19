import { createContext, useContext } from "react";

import { applyAddSongs, getAddSongsJob, getProPresenterPlaylists, lookupAddSongsTitles, previewAddSongs } from "../api";

/**
 * The five requests of the Presenter Link section. The app never provides another: the section asks StagePilot. The section gallery
 * provides made-up ones so every state (previewing, progress, a refusal) is reached by pressing the real button, with no backend.
 */
export interface PresenterApi {
  playlists: typeof getProPresenterPlaylists;
  titles: typeof lookupAddSongsTitles;
  preview: typeof previewAddSongs;
  apply: typeof applyAddSongs;
  job: typeof getAddSongsJob;
}

// Called by name at the moment of the request (so a test that replaces `../api` is heard).
export const stagePilotApi: PresenterApi = {
  playlists: (...args) => getProPresenterPlaylists(...args),
  titles: (...args) => lookupAddSongsTitles(...args),
  preview: (...args) => previewAddSongs(...args),
  apply: (...args) => applyAddSongs(...args),
  job: (...args) => getAddSongsJob(...args),
};

export const PresenterApiContext = createContext<PresenterApi>(stagePilotApi);
export const usePresenterApi = () => useContext(PresenterApiContext);
