<script setup lang="ts">
import { computed } from 'vue';
import { site, useContent } from '../content';
import LinkCard from './LinkCard.vue';
const props = defineProps<{ ids?: string[]; section?: string; kind?: 'api' }>();
const { pages } = useContent();
const cards = computed(() => {
  const ids = props.kind === 'api' ? site.navigation.apiRoutes.map(route => route.document) : props.ids;
  if (ids) return ids.map(id => pages.value.find(page => page.id === id)).filter(Boolean);
  return pages.value.filter(page => page.id !== 'overview' && page.section === props.section).sort((a, b) => a.order - b.order);
});
</script>
<template><div class="ds-grid"><LinkCard v-for="card in cards" :key="card!.id" :title="card!.title" :description="card!.description" :href="card!.route" /></div></template>
