import { defineConfig } from 'vitepress';
import { createCssVariablesTheme } from 'shiki';
import { fileURLToPath } from 'node:url';
import { readFileSync } from 'node:fs';
import { webNoticesPlugin } from '../scripts/web-notices.mjs';

const root = fileURLToPath(new URL('../../', import.meta.url));
const catalogue = JSON.parse(readFileSync(root + '.local/docs-site/catalogue.json', 'utf8'));
const { navigation, pages } = catalogue;
const siteTitle = pages.find((page: any) => page.id === 'getting-started' && page.locale === 'en').title;
function localeConfig(locale: string) {
  const settings = navigation.locales[locale];
  const ko = locale === 'ko';
  const sidebar = navigation.groups.map((group: any) => ({
    text: group[locale], link: settings.prefix + '/guide/sections/' + group.id,
    collapsed: group.id !== 'start' && group.id !== 'reference',
    items: pages.filter((page: any) => page.locale === locale && page.section === group.id && page.id !== 'overview')
      .sort((a: any, b: any) => a.order - b.order).map((page: any) => ({ text: page.title, link: page.route })),
  }));
  return {
    label: settings.label, lang: locale === 'en' ? 'en-US' : 'ko-KR', link: settings.prefix + '/',
    title: siteTitle,
    description: pages.find((page: any) => page.id === 'overview' && page.locale === locale).description,
    themeConfig: {
      nav: [
        { text: ko ? '문서' : 'Docs', link: settings.prefix + '/guide/getting-started', activeMatch: '^' + settings.prefix + '/guide/' },
        { text: ko ? 'API 레퍼런스' : 'API reference', link: settings.prefix + '/guide/sections/reference' },
        { text: ko ? '프로젝트' : 'Project', items: navigation.groups.slice(3).map((group: any) => ({ text: group[locale], link: settings.prefix + '/guide/sections/' + group.id })) },
        { text: 'GitHub', link: navigation.repository },
      ],
      sidebar: { [settings.prefix + '/guide/']: sidebar },
      outline: { level: [2, 3], label: ko ? '이 페이지에서' : 'On this page' },
      docFooter: { prev: ko ? '이전' : 'Previous', next: ko ? '다음' : 'Next' },
      sidebarMenuLabel: ko ? '메뉴' : 'Menu', returnToTopLabel: ko ? '맨 위로' : 'Return to top',
      darkModeSwitchLabel: ko ? '테마' : 'Appearance', lightModeSwitchTitle: ko ? '밝은 테마로 전환' : 'Switch to light theme',
      darkModeSwitchTitle: ko ? '어두운 테마로 전환' : 'Switch to dark theme', langMenuLabel: ko ? '언어 선택' : 'Change language',
      skipToContentLabel: ko ? '본문으로 건너뛰기' : 'Skip to content',
      notFound: { title: ko ? '페이지를 찾을 수 없습니다' : 'PAGE NOT FOUND', quote: ko ? '문서 탐색이나 검색으로 필요한 내용을 찾아보세요.' : 'Use the documentation or search to find your next step.', linkLabel: ko ? '홈으로' : 'Take me home', linkText: ko ? '홈으로 돌아가기' : 'Take me home' },
    },
  };
}

export default defineConfig({
  title: siteTitle,
  description: pages.find((page: any) => page.id === 'overview' && page.locale === 'en').description,
  base: navigation.base,
  srcDir: '../.local/docs-site/source',
  outDir: '../.local/docs-site/dist',
  cacheDir: '../.local/docs-site/cache',
  cleanUrls: true,
  // VitePress inserts local-search records as asynchronous page work completes.
  // Serial indexing keeps index IDs and artifact hashes stable across builds.
  buildConcurrency: 1,
  appearance: true,
  contentProps: { class: 'vp-doc' },
  locales: Object.fromEntries(Object.keys(navigation.locales).map(locale => [locale === 'en' ? 'root' : locale, localeConfig(locale)])),
  sitemap: undefined, // A production URL is assigned only after publication approval.
  markdown: {
    theme: createCssVariablesTheme({ name: 'gateway', variablePrefix: '--ds-code-' }),
    anchor: { slugify: (s: string) => s.toLowerCase().replace(/[^\p{L}\p{M}\p{N}\s_-]/gu, '').replace(/\s/g, '-') },
  },
  themeConfig: {
    search: {
      provider: 'local',
      options: {
        miniSearch: { searchOptions: { prefix: true, fuzzy: 0.2 } },
        locales: { ko: { translations: { button: { buttonText: '검색', buttonAriaLabel: '문서 검색' },
          modal: { displayDetails: '자세히 표시', resetButtonTitle: '검색 초기화', backButtonTitle: '검색 닫기',
            noResultsText: '검색 결과가 없습니다', footer: { selectText: '선택', navigateText: '이동', closeText: '닫기' } } } } },
      },
    },
  },
  vite: {
    plugins: [webNoticesPlugin(root)],
    resolve: { alias: [
      { find: /^vue$/, replacement: root + 'docs-site/node_modules/vue/dist/vue.runtime.esm-bundler.js' },
      { find: /^vue\/server-renderer$/, replacement: root + 'docs-site/node_modules/@vue/server-renderer/dist/server-renderer.esm-bundler.js' },
    ] },
    build: { sourcemap: false, emptyOutDir: true },
  },
});
