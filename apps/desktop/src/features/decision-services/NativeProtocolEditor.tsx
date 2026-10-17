import { useEffect, useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Disclosure } from '../../ui';
import type { DecisionService, Judgment, RouteBranch } from './types';
import { C_PROTOCOL, LEGACY_PROTOCOL, composedAssessment, contentDigest, freezeNative, nativeProtocol, nativeReferenceIssue, stockAssessment, type AssessmentReview, type AssessmentReviews, type CCapability, type NativeProtocol, type RollbackOption } from './native-protocol';

export type AssessmentScope = { id: string; label: string; judgment: Judgment };
export function NativeProtocolEditor({ protocol, reviews, scopes, service, branches, defaultId, sources = [], wasC = false, language, onProtocol, onScope }: {
  protocol?: NativeProtocol; reviews?: AssessmentReviews; scopes: AssessmentScope[]; service: DecisionService; branches?: RouteBranch[]; defaultId?: string; sources?: RollbackOption[]; wasC?: boolean; language: 'zh' | 'en';
  onProtocol(value?: NativeProtocol): void; onScope(id: string, judgment: Judgment, review?: AssessmentReview): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [choices, setChoices] = useState<Record<string, { method: AssessmentReview['method']; source?: RollbackOption['source'] }>>({});
  const [digests, setDigests] = useState<Record<string, string>>({});
  const [frozenDigest, setFrozenDigest] = useState('');
  const [result, setResult] = useState<{ key: string; value: CCapability }>();
  const [busy, setBusy] = useState(false), [failure, setFailure] = useState('');
  const current = useRef(''); current.current = JSON.stringify([protocol, scopes, service, branches, defaultId]);
  const key = current.current;
  useEffect(() => { let cancelled = false; void Promise.all(scopes.map(async s => [s.id, await contentDigest(composedAssessment(protocol, s.judgment))] as const)).then(v => { if (!cancelled) setDigests(Object.fromEntries(v)); }); return () => { cancelled = true; }; }, [key]);
  useEffect(() => { let cancelled = false; setFrozenDigest(''); if (protocol) void freezeNative(protocol, scopes[0].judgment, branches, defaultId).then(p => { if (!cancelled) setFrozenDigest(p.digest); }); return () => { cancelled = true; }; }, [key]);
  const destination = protocol ? C_PROTOCOL : LEGACY_PROTOCOL;
  const needsReview = !!protocol || !!reviews || wasC;
  const capability = result?.key === key ? result.value : undefined;
  async function review(scope: AssessmentScope) {
    const snapshot = key; setBusy(true);
    try {
      const digest = await contentDigest(composedAssessment(protocol, scope.judgment));
      if (current.current === snapshot) {
        const choice = choices[scope.id];
        const historical = choice?.method === 'historical' && sources.find(s => JSON.stringify(s.source) === JSON.stringify(choice.source) && s.instructions === scope.judgment.competence.instructions && JSON.stringify(s.criteria) === JSON.stringify(scope.judgment.competence.criteria));
        const stock = choice?.method === 'stock' && JSON.stringify(composedAssessment(protocol, scope.judgment)) === JSON.stringify(composedAssessment(protocol, stockAssessment(scope.judgment, protocol)));
        onScope(scope.id, scope.judgment, { protocol: destination, assessment_digest: digest, method: historical ? 'historical' : stock ? 'stock' : 'reviewed_custom', ...(historical ? { source: historical.source } : {}) });
      }
    } finally { setBusy(false); }
  }
  async function test() {
    if (!protocol) return;
    const snapshot = key; setBusy(true); setFailure('');
    try {
      const frozen = await freezeNative(protocol, scopes[0].judgment, branches, defaultId);
      const value = await invoke<CCapability>('test_classifier_decision', { input: { request: { schema: 'hiroute.classifier-decision-test/v2', service_id: service.id, service_revision: service.revision, protocol: frozen, shape: 'standard' } } });
      if (current.current === snapshot && value.protocol_digest === frozen.digest && value.service_id === service.id && value.service_revision === service.revision && service.connection.kind === 'system_one' && value.model === service.connection.model && value.endpoint_digest === await contentDigest(service.connection.endpoint)) setResult({ key: snapshot, value });
    } catch { setFailure(t('C 能力测试未完成。', 'C capability test did not complete.')); } finally { setBusy(false); }
  }
  return <section className="editor-section native-protocol-editor">
    <label className="field"><span className="field-label">{t('决策协议', 'Decision protocol')}</span><select className="input" value={protocol ? 'c' : 'legacy'} disabled={busy} onChange={e => onProtocol(e.target.value === 'c' ? nativeProtocol() : undefined)}><option value="legacy">{t('现有协议', 'Existing protocol')}</option><option value="c">{t('C · 历史问题适用性', 'C · historical problem applicability')}</option></select></label>
    {protocol && <><label className="field"><span className="field-label">{t('目标阶段之前的背景轮数', 'Background turns before the assessed stage')}</span><input className="input" type="number" min={0} max={8} step={1} value={protocol.prior_context_max_turns} onChange={e => onProtocol({ ...protocol, prior_context_max_turns: Number(e.target.value) })} /><span className="field-help">{t('默认 8 轮；不限制完整目标阶段的轮数。', 'Defaults to 8 turns; the complete assessed stage is not limited by this cap.')}</span></label>{protocol.prior_context_max_turns === 0 && <p role="status" className="field-help">{t('背景为 0：不会提供目标阶段之前的要求和引用。', 'With zero background, requirements and references before the assessed stage are not supplied.')}</p>}<p className="field-help">{t('C 保留当前难度和胜任度阈值。每个生效的评分范围都需要明确选择并审阅标准。', 'C preserves your degree and competence thresholds. Explicitly choose and review each effective assessment scope.')}</p></>}
    {needsReview && scopes.map(scope => {
      const question = composedAssessment(protocol, scope.judgment);
      const valid = reviews?.[scope.id]?.protocol === destination && reviews[scope.id].assessment_digest === digests[scope.id];
      const incompatible = nativeReferenceIssue(!!protocol, JSON.stringify(question));
      const source = sources.find(s => s.source.scope === scope.id);
      return <div className="native-assessment-scope" key={scope.id} data-native-scope={scope.id}>
        <h4>{scope.label}</h4><p className="field-help" role="status">{valid ? t('已审阅当前协议与完整评分标准', 'Current protocol and composed assessment reviewed') : t('需要审阅此范围的完整评分标准', 'Review the complete assessment for this scope')}</p>
        <div className="field-actions"><button type="button" className="btn" disabled={busy} onClick={() => { setChoices(c => ({ ...c, [scope.id]: { method: 'stock' } })); onScope(scope.id, stockAssessment(scope.judgment, protocol)); }}>{protocol ? t('采用标准 C 评分', 'Adopt stock C assessment') : t('采用旧默认评分', 'Adopt legacy default assessment')}</button>
          {!protocol && <button type="button" className="btn" disabled={busy || !source} title={!source ? t('没有可用的保留版本或匹配范围。', 'No retained source or matching scope is available.') : undefined} onClick={() => { if (!source) return; setChoices(c => ({ ...c, [scope.id]: { method: 'historical', source: source.source } })); onScope(scope.id, { ...scope.judgment, competence: { ...scope.judgment.competence, instructions: source.instructions, criteria: source.criteria } }); }}>{source ? t(`恢复版本 ${source.source.plan.content_revision} 的评分`, `Restore revision ${source.source.plan.content_revision} assessment`) : t('历史评分不可用', 'Historical assessment unavailable')}</button>}
        </div>
        <Disclosure label={t('审阅完整评分问题与标准', 'Review composed assessment and criteria')} language={language}><p className="native-prompt">{question.instructions}</p>{question.criteria.map((criterion, i) => <p className="native-prompt" key={i}>{i / 2} · {criterion}</p>)}<code className="native-content-digest">{digests[scope.id]}</code><button className="btn" type="button" disabled={busy || incompatible || valid} onClick={() => void review(scope)}>{t('已审阅，确认此范围', 'Reviewed: confirm this scope')}</button></Disclosure>
        {incompatible && <p className="field-error" role="alert">{t('此评分包含目标协议不支持的历史字段，请先编辑评分要求。', 'This assessment references history fields unavailable in the destination protocol. Edit the assessment first.')}</p>}
      </div>;
    })}
    {protocol && <><p className="field-help native-content-digest">{t('冻结内容摘要', 'Frozen content digest')} · {frozenDigest || '…'}</p><p role="status">{capability ? ({ verified: t('C 能力已验证', 'C capability verified'), unsupported: t('此形状的 C 响应不受支持', 'C response shape unsupported'), test_failed: t('C 能力测试失败', 'C capability test failed'), not_tested: t('本次打开尚未测试 C 能力', 'Not tested in this session') })[capability.state] : t('本次打开尚未测试 C 能力', 'Not tested in this session')}</p><p className="field-help">{t('缺少本次打开的能力证据仍可保存或发布。显式测试发送 1 次合成请求，可能消耗供应商配额；保存和发布不会测试。', 'Saving and publication remain available without current-session evidence. Explicit testing sends one synthetic request and may consume provider quota; saving and publishing do not test.')}</p><button className="btn" type="button" disabled={busy} onClick={() => void test()}>{t('测试此路由的 C 能力（1 次请求）', 'Test this route’s C capability (1 request)')}</button>{failure && <p role="alert">{failure}</p>}</>}
  </section>;
}
