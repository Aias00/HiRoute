import { Disclosure } from '../../ui';
import { trustLevels, trustLevelLabel } from './levels';
import type { Judgment } from '../decision-services/types';
import './trust.css';

export function TrustLevelControl({ value, onChange, language, id, complete, evidenceError }: {
  value: Judgment; onChange(value: Judgment): void; language: 'zh' | 'en'; id: string;
  complete?: number; evidenceError?: boolean;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const threshold = value.degree.simple_threshold_millis;
  const change = (simple_threshold_millis: number) => onChange({ ...value, degree: { ...value.degree, simple_threshold_millis } });
  return <div className="trust-control" data-trust-control={id}>
    <div className="editor-section-heading"><h3>{t('信任档位', 'Trust level')}</h3><span>{trustLevelLabel(threshold, language)}{trustLevels.some(level => level.threshold_millis === threshold) ? ` · ${threshold / 1000}` : ''}</span></div>
    <div className="trust-levels" role="group" aria-label={t('信任档位', 'Trust level')}>
      {trustLevels.map(level => <button key={level.id} type="button" className={`btn${threshold === level.threshold_millis ? ' selected' : ''}`} aria-pressed={threshold === level.threshold_millis} onClick={() => change(level.threshold_millis)}><strong>{level.label[language]}</strong><span>{level.threshold_millis / 1000}</span></button>)}
    </div>
    <p className="field-help">{t(`简单概率达到 ${Number.isFinite(threshold) ? threshold / 1000 : '—'} 时使用常规组。适用的完整评分低于 ${value.competence.floor_millis / 1000} 时，下一轮由主力组保护。`, `Use Regular when P(simple) reaches ${Number.isFinite(threshold) ? threshold / 1000 : '—'}. An applicable complete score below ${value.competence.floor_millis / 1000} protects the next turn with Primary.`)}</p>
    <p className="field-help" role="status">{complete === undefined ? (evidenceError ? t('暂时无法读取本修订版的证据。', 'Evidence for this revision is temporarily unavailable.') : t('本修订版的证据尚未载入。', 'Evidence for this revision has not loaded yet.')) : complete === 0 ? t('还没有记录的证据支持此档位。', 'No recorded evidence supports this level yet.') : t(`当前发布修订版有 ${complete} 个完整评分的常规阶段；档位是你的选择，证据支撑程度请看模型表现。`, `${complete} Regular stages have complete scores in the active revision. The level is your choice; see Model performance for the evidence.`)}</p>
    <Disclosure label={t('精确数值', 'Exact value')} language={language}><label className="field"><span className="field-label">{t('简单概率阈值', 'Simple probability threshold')}</span><input className="input" type="number" min={0} max={1} step={0.001} value={Number.isFinite(threshold) ? threshold / 1000 : ''} data-decision-field={`${id}-simple-threshold`} onChange={event => change(event.target.value === '' ? NaN : Math.round(Number(event.target.value) * 1000))} /></label></Disclosure>
    <p className="field-help">{t('信任档位只改变路由阈值。Agent 权限、工具审批和沙箱设置不变。保持当前档位也是正常选择。', 'Trust levels change only the routing threshold. Agent permissions, tool approval and sandbox settings are unchanged. Keeping this level is a valid choice.')}</p>
  </div>;
}
