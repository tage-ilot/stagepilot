import type { ApplicationState, ErrorSummary } from "../../types";

const CONNECTION_STATUS_BY_COMPONENT = {
  planning_center: "planning_center_status",
  midi: "midi_status",
  propresenter: "propresenter_status",
  lights: "lights_status",
} as const;

function errorIsActive(error: ErrorSummary, state: ApplicationState): boolean {
  if (error.component === "timer") {
    return state.timer.status === "error";
  }

  const connectionField =
    CONNECTION_STATUS_BY_COMPONENT[
      error.component as keyof typeof CONNECTION_STATUS_BY_COMPONENT
    ];
  if (connectionField) {
    return state[connectionField] === "error";
  }

  return state.plugins[error.component]?.status === "error";
}

export function latestActiveError(state: ApplicationState): ErrorSummary | null {
  for (let index = state.recent_errors.length - 1; index >= 0; index -= 1) {
    const error = state.recent_errors[index];
    if (error && errorIsActive(error, state)) {
      return error;
    }
  }
  return null;
}

