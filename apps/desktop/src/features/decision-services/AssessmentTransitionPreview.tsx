import type { AssessmentChoice, AssessmentTransition } from './native-protocol';
export function AssessmentTransitionPreview({ preview, busy, language, onChoice }: { preview: AssessmentTransition; busy: boolean; language: 'zh' | 'en'; onChoice(scope: string, choice: AssessmentChoice): void }) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  return <section className="editor-section native-protocol-editor" data-assessment-transition role="region" aria-label={t('协议切换发布预览', 'Protocol transition preview')}>
    <h3>{t('确认协议切换的评分标准', 'Confirm assessment standards for this transition')}</h3><p className="field-help">{t('以下内容由服务端生成。逐项选择后，检查最终正文并确认发布。', 'The server composed this content. Choose each scope, inspect the resulting text, then confirm publication.')}</p>
    {preview.scopes.map(scope => <div className="native-assessment-scope" data-native-scope={scope.scope} key={scope.scope}><h4>{scope.scope}</h4><p role="status">{scope.choice ? t('已选择，将随本次发布记录', 'Chosen for this publication') : t('请选择此范围的评分方式', 'Choose this scope’s assessment')}</p>
      <div className="field-actions"><button className="btn" type="button" disabled={busy} onClick={() => onChoice(scope.scope, { method: 'stock' })}>{t('采用目标方式的默认评分', 'Use destination default')}</button><button className="btn" type="button" disabled={busy} onClick={() => onChoice(scope.scope, { method: 'reviewed_custom' })}>{t('审阅并保留自定义评分', 'Review and keep custom assessment')}</button>{scope.historical_sources.map(source => <button type="button" className="btn" disabled={busy} key={source.source.plan.content_digest} onClick={() => onChoice(scope.scope, { method: 'historical', source: source.source })}>{t(`恢复版本 ${source.source.plan.content_revision} 的评分`, `Restore revision ${source.source.plan.content_revision} assessment`)}</button>)}</div>
      {!scope.historical_sources.length && <p className="field-help">{t('此范围没有可用的历史评分。', 'No retained historical assessment is available for this scope.')}</p>}
      <p className="native-prompt">{scope.assessment.instructions}</p>{scope.assessment.criteria.map((c,i) => <p className="native-prompt" key={i}>{i / 2} · {c}</p>)}<code className="native-content-digest">{scope.assessment_digest}</code>
    </div>)}
  </section>;
}
