export type TokenComponents = { input: number; cache_read: number; cache_write: number; output: number; total: number };
export type TokenShare = {
  basis: 'input_uncached+cache_read+cache_write+output';
  period: { kind: 'today' | 'last_7d'; timezone: string; start_ms: number; end_ms: number };
  agent_filter: string | null;
  by_billing_class: Record<'subscription' | 'paid' | 'free' | 'unknown', { tokens: TokenComponents; requests: number }>;
  by_group: Record<'regular' | 'primary' | 'other', { tokens: TokenComponents; turns: number }>;
  unknown_usage_requests: number;
  retention_boundary_partial: boolean;
  headline: { kind: 'share'; outside_subscription_ratio_millis: number; known_classified_tokens: number; unknown_usage_requests: number }
    | { kind: 'no_routed_requests' | 'no_subscription_in_period' | 'no_comparable_usage' }
    | { kind: 'subscription_usage_unknown'; unknown_usage_requests: number };
  disclosure: string;
};

export function tokenShareHeadline(share: TokenShare, language: 'zh' | 'en'): string {
  const t = (zh: string, en: string) => language === 'zh' ? zh : en;
  switch (share.headline.kind) {
    case 'share': return `${(share.headline.outside_subscription_ratio_millis / 10).toLocaleString(language === 'zh' ? 'zh-CN' : 'en', { maximumFractionDigits: 1 })}%`;
    case 'no_routed_requests': return t('此时段没有路由请求', 'No routed requests in this period');
    case 'no_subscription_in_period': return t('此时段未使用订阅来源', 'No subscription source used in this period');
    case 'subscription_usage_unknown': return t('订阅来源的用量未知', 'Subscription usage is unknown');
    case 'no_comparable_usage': return t('没有可比较的用量', 'No comparable usage');
  }
}
