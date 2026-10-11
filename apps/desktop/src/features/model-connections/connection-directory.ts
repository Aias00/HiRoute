import type { ComputeConnectionOption } from './types';

type Identity = { provider_id: string; name: { zh: string; en: string } };
export type DirectoryProduct = {
  option: ComputeConnectionOption;
  name: string;
  searchText: string;
};
export type DirectoryProvider = {
  id: string;
  name: string;
  products: DirectoryProduct[];
};
export type BillingFilter = 'all' | ComputeConnectionOption['billing_class'];

/** Unknown catalog entries remain visible even when the bundled branding has no match. */
export function connectionDirectory(
  options: ComputeConnectionOption[],
  language: 'zh' | 'en',
  identity: (optionId: string) => Identity | undefined,
): DirectoryProvider[] {
  const providers = new Map<string, DirectoryProvider>();
  for (const option of options) {
    if (option.origin === 'agent_subscription') continue;
    const template = identity(option.connection_option_id);
    const name = template?.name[language] || option.display_name;
    const providerName = name.split(' · ')[0];
    const providerId = template?.provider_id || option.connector_id || option.connection_option_id;
    const provider = providers.get(providerId) ?? { id: providerId, name: providerName, products: [] };
    provider.products.push({ option, name, searchText: [
      providerId, providerName, name, template?.name.zh, template?.name.en,
      option.display_name, option.connection_option_id,
      option.billing_class === 'subscription' && `${name} ${option.display_name}`.toLocaleLowerCase().includes('coding') ? 'coding plan' : '',
    ].filter(Boolean).join(' ').toLocaleLowerCase() });
    providers.set(providerId, provider);
  }
  return [...providers.values()];
}

export function filterConnectionDirectory(providers: DirectoryProvider[], query: string, billing: BillingFilter): DirectoryProvider[] {
  const needle = query.trim().toLocaleLowerCase();
  return providers.flatMap(provider => {
    const products = provider.products.filter(product =>
      (billing === 'all' || product.option.billing_class === billing)
      && (!needle || product.searchText.includes(needle)));
    return products.length ? [{ ...provider, products }] : [];
  });
}
