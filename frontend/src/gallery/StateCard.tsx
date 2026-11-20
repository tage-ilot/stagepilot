import type { ReactNode } from "react";

/** One state of one connection, on the gallery page: what it is, a line about what to look at, then the real block. */
export interface SampleState {
  id: string;
  label: string;
  /** What to look for in this state. */
  look?: string;
  render: () => ReactNode;
}

export function StateCard({ connection, state }: { connection: string; state: SampleState }) {
  const id = `${connection}-${state.id}`;
  return (
    <section aria-labelledby={`${id}-title`} className="min-w-0" data-sample={id}>
      <h3 className="text-sm font-semibold text-ink" id={`${id}-title`}>{state.label}</h3>
      {state.look && <p className="mb-1 text-xs text-ink-muted">{state.look}</p>}
      <div className="[&>section]:!mt-0">{state.render()}</div>
    </section>
  );
}

export function ConnectionGroup({ id, title, intro, states }: { id: string; title: string; intro: string; states: SampleState[] }) {
  return (
    <section aria-labelledby={`${id}-heading`} className="space-y-6" data-group={id}>
      <header className="border-t border-edge pt-4">
        <h2 className="text-xl font-bold text-ink" id={`${id}-heading`}>{title}</h2>
        <p className="text-sm text-ink-muted">{intro}</p>
        <ul className="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs text-ink-muted">
          {states.map((state) => <li key={state.id}><a className="underline can-hover:text-ink" href={`#${id}-${state.id}-title`}>{state.label}</a></li>)}
        </ul>
      </header>
      {states.map((state) => <StateCard connection={id} key={state.id} state={state} />)}
    </section>
  );
}
