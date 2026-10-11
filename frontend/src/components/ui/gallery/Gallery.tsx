import { ButtonSection } from "./ButtonSection";

export function Gallery() {
  return (
    <main data-ui-gallery className="min-h-screen space-y-6 bg-surface-950 p-4 font-type-body-md text-ink-body sm:p-6">
      <header className="space-y-2">
        <h1 className="text-2xl font-bold text-ink">StagePilot UI gallery</h1>
        <p>Review-only components. Nothing on this page controls a running show.</p>
      </header>
      <ButtonSection />
      <p className="text-sm text-ink-muted">Default · Hover · Focus · Active · Disabled · Loading · Error</p>
    </main>
  );
}
