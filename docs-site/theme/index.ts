import { h } from 'vue';
import type { Theme } from 'vitepress';
import DefaultTheme from 'vitepress/theme-without-fonts';
import LinkCard from './components/LinkCard.vue';
import CardGrid from './components/CardGrid.vue';
import StatusBadge from './components/StatusBadge.vue';
import Callout from './components/Callout.vue';
import SupportTable from './components/SupportTable.vue';
import DiagramFigure from './components/DiagramFigure.vue';
import ProductIntro from './components/ProductIntro.vue';
import PageToolbar from './components/PageToolbar.vue';
import SiteFooter from './components/SiteFooter.vue';
import './tokens.css';
import './styles.css';

export default {
  extends: DefaultTheme,
  Layout: () => h(DefaultTheme.Layout, null, {
    'doc-before': () => h(PageToolbar),
    'layout-bottom': () => h(SiteFooter),
  }),
  enhanceApp({ app }) {
    Object.entries({ LinkCard, CardGrid, StatusBadge, Callout, SupportTable, DiagramFigure, ProductIntro }).forEach(([name, component]) => app.component(name, component));
  },
} satisfies Theme;
