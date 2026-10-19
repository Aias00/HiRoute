import type { AgentActivationPreference } from './types';

export function ClaudeActivationChoice({ value, language, onChange }: {
  value: AgentActivationPreference; language: 'zh' | 'en'; onChange(value: AgentActivationPreference): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  return <label className="field" data-claude-activation-choice>
    <span className="field-label">{t('使用范围', 'Use routing for')}</span>
    <select className="select" value={value} onChange={event => onChange(event.target.value as AgentActivationPreference)}>
      <option value="automatic">{t('普通 Claude Code 会话', 'Ordinary Claude Code sessions')}</option>
      <option value="managed_launch">{t('仅通过 HiRoute 启动的会话', 'Only sessions started through HiRoute')}</option>
    </select>
    <span className="field-help">{value === 'managed_launch'
      ? t('保留原生登录和配置。保存后，使用 HiRoute 启动命令进入已映射路由。', 'Keep native sign-in and settings. After saving, use the HiRoute launch command for mapped routes.')
      : t('保存到 Claude Code 的原生配置，重新启动 claude 后生效。', 'Save to native Claude Code settings; restart claude to use the mapping.')}</span>
  </label>;
}
