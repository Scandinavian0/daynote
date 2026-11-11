<script lang="ts">
  // Round checkbox used by every task row and reminder card.
  let {
    checked,
    small = false,
    label,
    onchange,
  }: { checked: boolean; small?: boolean; label: string; onchange: () => void } = $props();
</script>

<button
  class="check"
  class:checked
  class:small
  role="checkbox"
  aria-checked={checked}
  aria-label={label}
  onclick={(e) => {
    e.stopPropagation();
    onchange();
  }}
>
  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="m6.5 12.5 3.7 3.7L17.500 8.500" /></svg>
</button>

<style>
  .check {
    flex: none;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    border: 1.5px solid rgba(255, 255, 255, 0.32);
    display: grid;
    place-items: center;
    transition: border-color 0.15s, background 0.18s var(--ease), transform 0.12s;
  }
  .check.small {
    width: 15px;
    height: 15px;
  }
  .check:hover {
    border-color: var(--accent);
    background: var(--accent-soft);
  }
  .check:active {
    transform: scale(0.88);
  }
  .check svg {
    width: 100%;
    height: 100%;
    fill: none;
    stroke: var(--accent-ink);
    stroke-width: 2.6;
    stroke-linecap: round;
    stroke-linejoin: round;
    stroke-dasharray: 18;
    stroke-dashoffset: 18;
    transition: stroke-dashoffset 0.22s var(--ease);
  }
  .check.checked {
    background: var(--accent);
    border-color: var(--accent);
  }
  .check.checked svg {
    stroke-dashoffset: 0;
  }
</style>
