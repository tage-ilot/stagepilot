import { ButtonSection } from "./ButtonSection";
import { DialogSection } from "./DialogSection";
import { DisclosureSection } from "./DisclosureSection";
import { FieldSection } from "./FieldSection";
import { PanelSection } from "./PanelSection";
import { SaveBarSection } from "./SaveBarSection";
import { ToastSection } from "./ToastSection";

export function Gallery() {
  return (
    <main data-ui-gallery className="min-h-screen space-y-6 p-4 font-type-body-md text-ink-body sm:p-6">
      <header className="space-y-2">
        <h1 className="text-2xl font-bold text-ink">StagePilot UI gallery</h1>
        <p>Review-only components. Nothing on this page controls a running show.</p>
      </header>
      <PanelSection />
      <DisclosureSection />
      <SaveBarSection />
      <DialogSection />
      <ToastSection />
      <ButtonSection />
      <FieldSection />
      <p className="text-sm text-ink-muted">Default · Hover · Focus · Active · Disabled · Loading · Error</p>
    </main>
  );
}
