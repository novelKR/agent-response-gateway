<script setup lang="ts">
import { withBase } from 'vitepress';
import { site, useContent } from '../content';
const { ui } = useContent();
</script>
<template><div class="ds-footer"><span>{{ ui.stage }}</span><span>{{ site.dirty ? ui.preview : ui.source }} · <a :href="site.navigation.repository + '/commit/' + site.commit">{{ site.commit.slice(0, 7) }}</a></span><a :href="withBase('/web-notices.txt')">{{ ui.notices }}</a><a :href="withBase('/LICENSE.txt')">AGPL-3.0-only</a></div></template>
