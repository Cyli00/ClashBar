// 当前 CHANGELOG 保存的是上游历史；旧版本和 Issue 不能改指向新客户端仓库。
const historicalRepository = 'Sitoi/ClashBar';

export type ChangelogCategory = 'feature' | 'improve' | 'fix';

export type ChangelogItem = {
  text: string;
};

export type ChangelogRelease = {
  version: string;
  summary: string;
  categories: Record<ChangelogCategory, ChangelogItem[]>;
};

export type ChangelogData = {
  ok: boolean;
  error?: string;
  releases: ChangelogRelease[];
  generatedAt: string;
  source: string;
};

export const CHANGELOG_CATEGORY_META: Record<
  ChangelogCategory,
  { label: string; shortLabel: string; tone: string }
> = {
  feature: {
    label: '新增功能',
    shortLabel: '功能',
    tone: 'feature',
  },
  improve: {
    label: '优化改进',
    shortLabel: '优化',
    tone: 'improve',
  },
  fix: {
    label: '修复问题',
    shortLabel: '修复',
    tone: 'fix',
  },
};

export const CHANGELOG_CATEGORIES: ChangelogCategory[] = [
  'feature',
  'improve',
  'fix',
];

export function githubReleaseUrl(version: string): string {
  const tag = version.startsWith('v') ? version : `v${version}`;
  return `https://github.com/${historicalRepository}/releases/tag/${tag}`;
}

export function githubReleasesUrl(): string {
  return `https://github.com/${historicalRepository}/releases`;
}

export function githubIssueUrl(issue: number): string {
  return `https://github.com/${historicalRepository}/issues/${issue}`;
}

/** Count items in a release that match the active category filter. */
export function countFilteredItems(
  release: ChangelogRelease,
  active: ReadonlySet<ChangelogCategory>,
): number {
  let total = 0;
  for (const key of CHANGELOG_CATEGORIES) {
    if (!active.has(key)) continue;
    total += release.categories[key].length;
  }
  return total;
}
