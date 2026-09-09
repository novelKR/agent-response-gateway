<script setup lang="ts">
import { computed, ref, onMounted, onUnmounted } from 'vue';
import { useData, withBase } from 'vitepress';
import { site, useContent } from '../content';
import StatusBadge from './StatusBadge.vue';
const { frontmatter } = useData();
const { locale, ui } = useContent();
const hash = ref('');
const updateHash = () => { hash.value = window.location.hash; };
onMounted(() => { updateHash(); window.addEventListener('hashchange', updateHash); });
onUnmounted(() => window.removeEventListener('hashchange', updateHash));
const pairs = computed(() => site.pages.filter(page => page.id === frontmatter.value.docId));
</script>
<template>
  <div class="ds-toolbar" v-if="pairs.length">
    <StatusBadge>{{ ui.stage }}</StatusBadge>
    <nav class="ds-languages" :aria-label="ui.language">
      <a v-for="page in pairs" :key="page.locale" :href="withBase(page.route) + hash" :lang="page.locale" :aria-current="page.locale === locale ? 'page' : undefined">{{ site.navigation.locales[page.locale].label }}</a>
    </nav>
  </div>
</template>
