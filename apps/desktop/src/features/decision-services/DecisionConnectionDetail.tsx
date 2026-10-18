import { Disclosure, UiIcon } from '../../ui';
import { useState } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { stockProbe, contentDigest, type CCapability, type CapabilityShape } from './native-protocol';
import type { Draft, Plan } from '../../plan-editor';
import type { DecisionService } from './types';
import { decisionFailureHelp, decisionReferences, providerLabel, type DecisionTest } from './presentation';
import { DecisionIcon } from './DecisionIcon';

export function DecisionConnectionDetail({ service, language, mutable, busy, result, savedBeforeTest, plans, drafts, onEdit, onTest, onDelete, onProtocol, onOpenRoute, onUse }: {
  service: DecisionService; language: 'zh' | 'en'; mutable: boolean; busy: boolean; result?: DecisionTest; savedBeforeTest: boolean;
  plans: Plan[]; drafts: Draft[]; onEdit(): void; onTest(): void; onDelete(): void; onProtocol(): void;
  onOpenRoute?(plan?: Plan, draft?: Draft): void; onUse?(): void;
}) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  const connection = service.connection;
  const references = decisionReferences(service.id, plans, drafts);
  const [cResult, setCResult] = useState<{ key: string; value: CCapability }>();
  const [selectedShape, setSelectedShape] = useState<CapabilityShape | 'connectivity'>('connectivity');
  const [cBusy, setCBusy] = useState(false), [cError, setCError] = useState('');
  const cKey = JSON.stringify(service);
  const capability = cResult?.key === cKey ? cResult.value : undefined;
  async function testC(shape: CapabilityShape) {
    setCBusy(true); setCError('');
    try {
      const request = await stockProbe(service, shape);
      const value = await invoke<CCapability>('test_classifier_decision', { input: { request } });
      if (service.connection.kind === 'system_one' && value.protocol_digest === request.target.protocol.digest && value.service_id === service.id && value.service_revision === service.revision && value.endpoint_digest === await contentDigest(service.connection.endpoint) && value.model === service.connection.model) setCResult({ key: cKey, value });
    } catch { setCError(t('适用性测试未完成。', 'Applicability test did not complete.')); } finally { setCBusy(false); }
  }
  return <>
    <header className="editor-section decision-heading"><DecisionIcon provider={connection.kind === 'system_one' ? connection.provider : undefined} language={language} /><div><h2>{service.name}</h2><p>{providerLabel(connection, language)}</p></div><button className="btn" disabled={!mutable || busy} onClick={onEdit}>{t('编辑', 'Edit')}</button></header>
    <section className="editor-section"><dl className="decision-facts"><div><dt>{t('类型', 'Type')}</dt><dd>{connection.kind === 'system_one' ? t('决策模型 · HiRoute 内置逻辑', 'Decision model · HiRoute logic') : t('自定义扩展 · 自行实现决策', 'Custom extension · your decision logic')}</dd></div>{connection.kind === 'system_one' && <div><dt>{t('模型', 'Model')}</dt><dd>{connection.model}</dd></div>}<div><dt>{t('接入点', 'Endpoint')}</dt><dd className="decision-endpoint">{connection.endpoint}</dd></div><div><dt>{t('认证', 'Authentication')}</dt><dd>{connection.auth_header ? t('已配置', 'Configured') : t('无需认证', 'No authentication')}</dd></div></dl>
      {connection.kind === 'custom' && <div className="decision-protocol-intro"><p>{t('扩展负责模型接入、分支判断和评分；HiRoute 校验结果并执行路由、升级与回退。', 'The extension owns integration, branch selection and scoring. HiRoute validates the result and handles routing, upgrades and fallback.')}</p><button className="btn" onClick={onProtocol}>{t('查看接入协议', 'View protocol')}</button></div>}
    </section>
    <section className="editor-section"><div className={'decision-test-result' + (result ? result.passed ? ' passed' : ' failed' : '')} role="status"><UiIcon name={busy ? 'clock' : result ? result.passed ? 'check' : 'warning' : 'plug'} /><div><strong>{busy ? t('正在处理连接…', 'Processing connection…') : result ? result.passed ? t('连接测试通过', 'Connection test passed') : savedBeforeTest ? t('配置已保存，测试失败', 'Configuration saved; test failed') : t('连接测试失败', 'Connection test failed') : t('本次打开尚未测试', 'Not tested during this visit')}</strong>{result && <p>{result.passed ? t(`配置版本 ${service.revision} · ${result.duration ?? '—'} ms · ${new Date(result.at).toLocaleTimeString()}`, `Configuration ${service.revision} · ${result.duration ?? '—'} ms · ${new Date(result.at).toLocaleTimeString()}`) : decisionFailureHelp(result.code ?? '', language)}</p>}{result?.code && <Disclosure label={t('诊断详情', 'Diagnostic details')} language={language}><code>{result.code}</code></Disclosure>}</div></div><div className="decision-test-actions">{connection.kind === 'system_one' && <select className="select" value={selectedShape} aria-label={t('测试请求形状', 'Test request shape')} onChange={e => setSelectedShape(e.target.value as CapabilityShape | 'connectivity')}><option value="connectivity">{t('连接 · 2 个问题', 'Connectivity · 2 questions')}</option><option value="standard">{t('历史问题适用性 · 3 个问题', 'Historical applicability · 3 questions')}</option><option value="max_branches">{t('最大形状 · 19 个问题', 'Maximum · 19 questions')}</option></select>}<button className="btn" disabled={!mutable || busy || cBusy} onClick={() => selectedShape === 'connectivity' || connection.kind === 'custom' ? onTest() : void testC(selectedShape)}><UiIcon name="refresh" />{selectedShape === 'connectivity' ? t('测试连接', 'Test connection') : cBusy ? t('正在测试…', 'Testing…') : t('测试所选形状', 'Test selected shape')}</button>{result && !result.passed && <button className="btn" disabled={!mutable || busy} onClick={onEdit}>{t('修改连接', 'Edit connection')}</button>}{onUse && <button className="btn btn-primary" disabled={busy} onClick={onUse}>{t('用于当前路由', 'Use in current route')}</button>}</div><p className="field-help">{t('发送固定示例验证连接，可能消耗供应商配额；不读取真实会话。测试结果仅对应此配置版本，不代表持续可用或实际路由质量。', 'Sends a fixed example to check the connection and may consume provider quota; no real conversation is read. Results apply to this version and do not establish ongoing availability or routing quality.')}</p></section>
    {connection.kind === 'system_one' && selectedShape !== 'connectivity' && <section className="editor-section native-protocol-editor"><h3>{t('历史问题适用性', 'Historical problem applicability')}</h3><p role="status">{capability ? ({ verified: t('适用性请求形状已验证', 'Applicability request shape verified'), unsupported: t('此请求形状不受支持', 'Requested shape unsupported'), not_tested: t('本次打开尚未测试历史问题适用性', 'Not tested in this session'), test_failed: t('适用性测试失败', 'Applicability test failed') })[capability.state] : t('本次打开尚未测试历史问题适用性', 'Not tested in this session')}</p>
      {capability && <p className="field-help native-content-digest">{capability.question_count} {t('个问题', 'questions')} · {capability.duration_millis} ms · {capability.protocol_digest}</p>}
      <p className="field-help">{t('测试操作显式发送 1 次合成请求，可能消耗供应商配额。3 个问题的结果不代表 19 个问题的容量；自定义路由需在路由编辑器中测试实际冻结内容。保存与发布不会测试。', 'The test action explicitly sends one synthetic request and may consume provider quota. Three questions do not establish nineteen-question capacity; test custom route content in the route editor. Saving and publishing do not test.')}</p>
      {cError && <p role="alert">{cError}</p>}
    </section>}
    <section className="editor-section"><h3>{t('用于这些路由', 'Used by these routes')}</h3>{references.length ? <div className="decision-references">{references.map(reference => <button className="decision-reference" key={reference.key} onClick={() => onOpenRoute?.(reference.plan, reference.draft)} disabled={!onOpenRoute || busy}><UiIcon name="route" /><span>{reference.name || t('未命名草稿', 'Untitled draft')}<small>{reference.draft ? t('草稿', 'Draft') : t('已发布', 'Published')} · {t(`配置版本 ${reference.revision}`, `Configuration ${reference.revision}`)}</small></span><UiIcon name="chevronRight" /></button>)}</div> : <p className="field-help">{t('当前路由和草稿尚未使用此连接。', 'No current route or draft uses this connection.')}</p>}<p className="field-help">{t('更改连接后，需在路由中选用新版本并发布。此处不包含保留的历史版本引用。', 'After editing a connection, select the new version in a route and publish it. Retained historical references are not listed here.')}</p></section>
    <section className="editor-section"><Disclosure label={t('连接详情与更多操作', 'Connection details and more actions')} language={language}><dl className="decision-facts"><div><dt>{t('配置版本', 'Configuration version')}</dt><dd>{service.revision}</dd></div><div><dt>{t('决策超时', 'Decision timeout')}</dt><dd>{connection.timeout_ms / 1000} {t('秒', 'seconds')}</dd></div></dl><button className="btn btn-danger" disabled={!mutable || busy} onClick={onDelete}><UiIcon name="trash" />{t('删除连接', 'Delete connection')}</button></Disclosure></section>
  </>;
}
