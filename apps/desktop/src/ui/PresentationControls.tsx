import type { Language, LanguagePreference, ThemePreference } from './preferences';

const copy = {
  zh: {
    language: '显示语言', theme: '外观', system: '跟随系统', light: '浅色', dark: '深色',
  },
  en: {
    language: 'Display language', theme: 'Appearance', system: 'System', light: 'Light', dark: 'Dark',
  },
};

export function PresentationControls({
  language,
  languagePreference,
  theme,
  onLanguageChange,
  onThemeChange,
}: {
  language: Language;
  languagePreference: LanguagePreference;
  theme: ThemePreference;
  onLanguageChange: (language: LanguagePreference) => void;
  onThemeChange: (theme: ThemePreference) => void;
}) {
  const text = copy[language];
  return (
    <div className="hr-preferences">
      <label>
        <span>{text.language}</span>
        <select value={languagePreference} onChange={event => onLanguageChange(event.target.value as LanguagePreference)}>
          <option value="system">{text.system}</option>
          <option value="zh">简体中文</option>
          <option value="en">English</option>
        </select>
      </label>
      <fieldset>
        <legend>{text.theme}</legend>
        <div className="hr-segmented">
          {(['system', 'light', 'dark'] as ThemePreference[]).map(value => (
            <button key={value} type="button" aria-pressed={theme === value} onClick={() => onThemeChange(value)}>
              {text[value]}
            </button>
          ))}
        </div>
      </fieldset>
    </div>
  );
}
