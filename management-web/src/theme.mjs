export const themeChoices = Object.freeze(['light', 'dark', 'system']);
export const themeStorageKey = 'gateway-view-theme';

// Presentation preferences only. No authentication or server state belongs here.
export function createTheme({ root, media, storage, initial } = {}) {
  let choice = 'light';
  try { const saved = storage?.getItem(themeStorageKey); if (themeChoices.includes(saved)) choice = saved; } catch {}
  if (themeChoices.includes(initial)) choice = initial;
  const apply = () => { root.dataset.theme = choice === 'system' ? media.matches ? 'dark' : 'light' : choice; };
  const changed = () => { if (choice === 'system') apply(); };
  media.addEventListener('change', changed);
  apply();
  return Object.freeze({
    get choice() { return choice; },
    set(value) {
      if (!themeChoices.includes(value)) throw new TypeError('Unknown theme preference');
      choice = value; apply();
      try { storage?.setItem(themeStorageKey, value); } catch {}
    },
    dispose() { media.removeEventListener('change', changed); },
  });
}
