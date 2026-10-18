import { formatMoney } from '../../ui';

export type TrustCostValue = { kind: 'unavailable'; reason: string } | {
  kind: 'available'; difference_micros: number | null;
  comparison: { currency: string; coverage: 'complete' | 'partial' | 'unknown'; request_count: number; compared_request_count: number; missing_request_count: number; excluded_request_count: number; routed_estimate_micros: number | null; reference_estimate_micros: number | null; compared_routed_subtotal_micros: number | null; compared_reference_subtotal_micros: number | null; retention_boundary_partial: boolean; arithmetic_overflow: boolean };
};

export function TrustCost({ value, language }: { value: TrustCostValue; language: 'zh' | 'en' }) {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  if (value.kind === 'unavailable') {
    const reason = value.reason === 'primary_not_unique' ? t('需要唯一的已发布主力模型。', 'A unique published Primary model is required.')
      : value.reason === 'currency_not_unique' ? t('需要唯一的可用 API 参考币种。', 'One available API-reference currency is required.')
      : value.reason === 'prices_unavailable' || value.reason === 'reference_unavailable' ? t('当前来源的 API 参考价格不可用。', 'Current source-specific API reference prices are unavailable.')
      : t('记录正在准备或证据不足，请稍后重试。', 'Records are catching up or evidence is unavailable. Retry later.');
    return <p className="field-help">{t('成本对比暂不可用：', 'Cost comparison unavailable: ')}{reason}</p>;
  }
  const c = value.comparison;
  const amount = (micros: number | null) => micros === null ? t('未知', 'Unknown') : formatMoney(c.currency, (micros / 1_000_000).toFixed(4));
  const partial = c.coverage !== 'complete';
  return <article className="trust-evidence-row" aria-label={t('常规组成本对比', 'Regular cost comparison')}>
    <strong>{t('常规组成本对比 · API 等价值', 'Regular cost comparison · API-equivalent estimate')}</strong>
    <p>{t('路由估算', 'Routed estimate')}: {amount(c.routed_estimate_micros)} · {t('主力参考', 'Primary reference')}: {amount(c.reference_estimate_micros)}</p>
    <p>{value.difference_micros === null ? t('完整范围的差额未知。', 'The full-scope difference is unknown.') : <>{value.difference_micros > 0 ? t('额外成本', 'Additional cost') : t('估算差额', 'Estimated difference')}: {value.difference_micros > 0 ? '+' : value.difference_micros < 0 ? '−' : ''}{amount(Math.abs(value.difference_micros))}</>}</p>
    <p>{t(`覆盖 ${c.compared_request_count}/${c.request_count} 个请求 · 缺失 ${c.missing_request_count} · 排除 ${c.excluded_request_count}`, `Coverage: ${c.compared_request_count}/${c.request_count} requests · ${c.missing_request_count} missing · ${c.excluded_request_count} excluded`)}</p>
    {partial && c.compared_request_count > 0 && <p>{t('仅完整配对请求的小计', 'Paired complete-request subtotals')}: {amount(c.compared_routed_subtotal_micros)} / {amount(c.compared_reference_subtotal_micros)}</p>}
    {(c.retention_boundary_partial || c.arithmetic_overflow) && <p className="field-help">{c.arithmetic_overflow ? t('金额超出计算范围。', 'An amount exceeds the arithmetic range.') : t('部分记录已超出保留期限。', 'Some records are outside retention.')}</p>}
    <p className="field-help">{t('按所选 Plan 修订版、分类和常规组筛选，沿用已记录 token，包含重试，排除决策模型调用。参考模型实际使用时可能产生不同 token 和缓存行为。这是估算，不是账单或现金节省证明。', 'Scoped to the selected Plan revision, category and Regular group, using recorded tokens, including retries and excluding decision-model calls. The reference model could produce different tokens and cache behavior. This estimate does not establish a bill or cash saving.')}</p>
  </article>;
}
