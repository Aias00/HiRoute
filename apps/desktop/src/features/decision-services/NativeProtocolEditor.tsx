import { useRef, useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { Disclosure } from '../../ui';
import type { DecisionService, Judgment, RouteBranch } from './types';
import { composedAssessment, contentDigest, freezeNative, nativeProtocol, type CCapability, type NativeProtocol } from './native-protocol';

export type AssessmentScope = { id: string; label: string; judgment: Judgment };
export function NativeProtocolEditor({ protocol, scopes, service, branches, defaultId, language, onProtocol }: {
  protocol?: NativeProtocol; scopes: AssessmentScope[]; service: DecisionService; branches?: RouteBranch[]; defaultId?: string; language: 'zh' | 'en';
  onProtocol(value?: NativeProtocol): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const [result, setResult] = useState<{ key: string; value: CCapability }>();
  const [busy, setBusy] = useState(false), [failure, setFailure] = useState('');
  const key = JSON.stringify([protocol, scopes, service, branches, defaultId]);
  const current = useRef(key); current.current = key;
  const capability = result?.key === key ? result.value : undefined;
  async function test() {
    if (!protocol || !scopes.length) return;
    setBusy(true); setFailure(''); const requested = key;
    try {
      const frozen = await freezeNative(protocol, scopes.find(s => s.id === 'default')?.judgment ?? scopes[0].judgment, branches, defaultId);
      const value = await invoke<CCapability>('test_classifier_decision', { input: { request: { schema: 'hiroute.classifier-decision-test/v2', target: { kind: 'saved', service_id: service.id, service_revision: service.revision, protocol: frozen, shape: 'standard' } } } });
      if (current.current === requested && value.protocol_digest === frozen.digest && value.service_id === service.id && value.service_revision === service.revision && service.connection.kind === 'system_one' && value.endpoint_digest === await contentDigest(service.connection.endpoint) && value.model === service.connection.model) setResult({ key: requested, value });
    } catch { setFailure(t('适用性测试未完成。', 'Applicability test did not complete.')); } finally { setBusy(false); }
  }
  return <section className="editor-section native-protocol-editor">
    <h3>{t('历史问题适用性', 'Historical problem applicability')}</h3>
    <label className="field"><span className="field-label">{t('判断方式', 'Decision behavior')}</span><select className="select" value={protocol ? 'applicability' : 'legacy'} onChange={e => onProtocol(e.target.value === 'applicability' ? nativeProtocol() : undefined)}><option value="legacy">{t('现有判断方式', 'Existing behavior')}</option><option value="applicability">{t('历史问题适用性', 'Historical problem applicability')}</option></select></label>
    <p className="field-help">{t('切换只修改草稿。发布预览会列出受影响的评分范围，确认后才会更新路由；同一方式下的普通编辑无需重新审阅协议。', 'Selection changes only the draft. Publication Preview lists affected assessment scopes for confirmation; ordinary edits within the same behavior need no protocol review.')}</p>
    {protocol && <><label className="field"><span className="field-label">{t('目标阶段之前的背景轮数', 'Background turns before the assessed stage')}</span><input className="input" type="number" min={0} max={8} step={1} value={protocol.prior_context_max_turns} onChange={e => onProtocol({ ...protocol, prior_context_max_turns: Number(e.target.value) })} /><span className="field-help">{t('默认 8 轮；不限制完整目标阶段的轮数。', 'Defaults to 8 turns; the complete assessed stage is not limited by this cap.')}</span></label>{protocol.prior_context_max_turns === 0 && <p role="status" className="field-help">{t('背景为 0：不会提供目标阶段之前的要求和引用。', 'With zero background, requirements and references before the assessed stage are not supplied.')}</p>}</>}
    {scopes.map(scope => <Disclosure key={scope.id} label={`${scope.label} · ${t('完整评分问题与标准', 'Composed assessment and criteria')}`} language={language}><p className="native-prompt">{composedAssessment(protocol, scope.judgment).instructions}</p>{scope.judgment.competence.criteria.map((c, i) => <p className="native-prompt" key={i}>{i / 2} · {c}</p>)}</Disclosure>)}
    {protocol && <><p role="status">{capability ? ({ verified: t('适用性请求形状已验证', 'Applicability request shape verified'), unsupported: t('此请求形状不受支持', 'Requested shape unsupported'), test_failed: t('适用性测试失败', 'Applicability test failed'), not_tested: t('本次打开尚未测试历史问题适用性', 'Not tested in this session') })[capability.state] : t('本次打开尚未测试历史问题适用性', 'Not tested in this session')}</p><p className="field-help">{t('缺少本次打开的能力证据仍可保存或发布。显式测试发送 1 次合成请求，可能消耗供应商配额；保存和发布不会测试。', 'Saving and publication remain available without current-session evidence. Explicit testing sends one synthetic request and may consume provider quota; saving and publishing do not test.')}</p><button className="btn" type="button" disabled={busy} onClick={() => void test()}>{t('测试此路由的适用性（1 次请求）', 'Test route applicability (1 request)')}</button>{failure && <p role="alert">{failure}</p>}</>}
  </section>;
}
