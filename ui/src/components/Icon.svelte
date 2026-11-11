<script lang="ts">
  // Small stroke icons drawn on a 24px grid.
  const paths: Record<string, string> = {
    "chevron-left": '<path d="m15 18-6-6 6-6"/>',
    "chevron-right": '<path d="m9 18 6-6-6-6"/>',
    "chevron-down": '<path d="m6 9 6 6 6-6"/>',
    pin: '<path d="M12 16v5"/><path d="M9 4h6l-.8 5.5L17 12.5V15H7v-2.5l2.8-3z"/>',
    sidebar: '<rect x="3" y="4.5" width="18" height="15" rx="3"/><path d="M9.5 4.5v15"/>',
    sliders: '<path d="M4 7h9M17 7h3M4 17h3M11 17h9"/><circle cx="15" cy="7" r="2"/><circle cx="9" cy="17" r="2"/>',
    x: '<path d="M18 6 6 18M6 6l12 12"/>',
    minus: '<path d="M6 12h12"/>',
    bell: '<path d="M6.5 9a5.5 5.5 0 0 1 11 0c0 6 2.5 7.5 2.5 7.5H4S6.5 15 6.5 9"/><path d="M10.2 20a2 2 0 0 0 3.6 0"/>',
    flag: '<path d="M6 21V4"/><path d="M6 4.5h11l-2.2 3.8L17 12H6"/>',
    repeat: '<path d="m17 3 3.5 3.5L17 10"/><path d="M3.5 11v-.5a4 4 0 0 1 4-4h13"/><path d="m7 21-3.5-3.5L7 14"/><path d="M20.500 13v.5a4 4 0 0 1-4 4h-13"/>',
    trash: '<path d="M4 7h16"/><path d="M9 7V4.500h6V7"/><path d="m18 7-.8 12.5H6.800L6 7"/>',
    plus: '<path d="M12 5v14M5 12h14"/>',
    check: '<path d="m5 12.500 4.500 4.500L19 7"/>',
    grip: '<circle cx="9" cy="7" r=".6"/><circle cx="15" cy="7" r=".6"/><circle cx="9" cy="12" r=".6"/><circle cx="15" cy="12" r=".6"/><circle cx="9" cy="17" r=".6"/><circle cx="15" cy="17" r=".6"/>',
    note: '<path d="M5 4.500h14v10.500l-4.500 4.500H5z"/><path d="M14.500 19.500V15H19"/>',
    "arrow-right": '<path d="M5 12h14"/><path d="m13 6 6 6-6 6"/>',
    undo: '<path d="M9 14 4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
    copy: '<rect x="9" y="9" width="11" height="11" rx="2.500"/><path d="M5 15V6.500A2.500 2.500 0 0 1 7.500 4H15"/>',
    sun: '<circle cx="12" cy="12" r="3.500"/><path d="M12 3v2M12 19v2M3 12h2M19 12h2M5.600 5.600 7 7M17 17l1.400 1.400M5.600 18.400 7 17M17 7l1.400-1.400"/>',
    moon: '<path d="M20 14.500A8 8 0 0 1 9.500 4a8 8 0 1 0 10.500 10.500z"/>',
  };
  let { name, size = 16 }: { name: string; size?: number } = $props();
</script>

<svg
  width={size}
  height={size}
  viewBox="0 0 24 24"
  fill="none"
  stroke="currentColor"
  stroke-width="1.7"
  stroke-linecap="round"
  stroke-linejoin="round"
  aria-hidden="true"
>
  {@html paths[name] ?? ""}
</svg>

<style>
  svg {
    display: block;
    flex: none;
  }
</style>
