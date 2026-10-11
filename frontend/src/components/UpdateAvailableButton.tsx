import { forwardRef } from "react";

import { Button } from "./ui";

export const UpdateAvailableButton = forwardRef<HTMLButtonElement, {
  version: string;
  onClick: () => void;
}>(({ version, onClick }, ref) => (
  <Button
    aria-label={`Update StagePilot to version ${version}`}
    className="ml-5 gap-1.5 font-bold"
    onClick={onClick}
    ref={ref}
    title={`StagePilot ${version} is available`}
    tone="info"
    variant="tinted"
  >
    <svg aria-hidden="true" className="h-3.5 w-3.5" fill="none" viewBox="0 0 24 24">
      <path d="M12 3v12m0 0 4-4m-4 4-4-4M5 19h14" stroke="currentColor" strokeLinecap="round" strokeLinejoin="round" strokeWidth="2" />
    </svg>
    Update
  </Button>
));

UpdateAvailableButton.displayName = "UpdateAvailableButton";
