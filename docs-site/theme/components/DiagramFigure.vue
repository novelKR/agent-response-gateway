<script setup lang="ts">
import { computed } from 'vue';
import { site, useContent } from '../content';
const props = withDefaults(defineProps<{ kind?: 'routes' | 'ownership' | 'tool-roundtrip' }>(), { kind: 'routes' });
const { ui } = useContent();
const nodes = computed(() => props.kind === 'ownership'
  ? [{ title: ui.value.consumer, detail: ui.value.tools }, { title: ui.value.gateway, detail: ui.value.transport }, { title: ui.value.upstream, detail: ui.value.wire }]
  : [{ title: ui.value.consumer, detail: 'POST /v1/responses' }, { title: ui.value.gateway, detail: ui.value.transport }]);
</script>
<template>
  <figure class="ds-figure" :data-kind="kind">
    <div v-if="kind === 'tool-roundtrip'" class="ds-sequence">
      <div class="ds-sequence-actors"><strong>{{ ui.consumer }}</strong><strong>{{ ui.gateway }}</strong><strong>{{ ui.upstream }}</strong></div>
      <ol>
        <li v-for="(step, index) in ui.toolSteps" :key="step" :data-direction="index === 2 ? 'host' : index === 1 || index === 4 ? 'return' : 'forward'">
          <span>{{ index + 1 }}. {{ step }}</span>
          <svg v-if="index !== 2" viewBox="0 0 600 28" aria-hidden="true"><path d="M100 14H500m-8-8 8 8-8 8" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
        </li>
      </ol>
    </div>
    <ol v-else class="ds-flow">
      <li v-for="(node, index) in nodes" :key="node.title">
        <svg v-if="index" class="ds-flow-arrow" viewBox="0 0 32 24" aria-hidden="true"><path d="M16 1v19m-5-5 5 5 5-5" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
        <div class="ds-flow-node" :data-gateway="index === 1"><strong>{{ node.title }}</strong><span>{{ node.detail }}</span></div>
      </li>
    </ol>
    <div v-if="kind === 'routes'" class="ds-route-options">
      <svg class="ds-flow-arrow" viewBox="0 0 32 24" aria-hidden="true"><path d="M16 1v19m-5-5 5 5 5-5" fill="none" stroke="currentColor" stroke-width="1.5" /></svg>
      <p class="ds-route-label">{{ ui.upstream }}</p>
      <ul>
        <li v-for="route in site.navigation.apiRoutes" :key="route.document" class="ds-flow-node"><strong>{{ route.label }}</strong><code>{{ route.endpoint }}</code></li>
      </ul>
    </div>
    <figcaption>{{ kind === 'routes' ? ui.routeCaption : kind === 'tool-roundtrip' ? ui.toolCaption : ui.ownershipCaption }}</figcaption>
  </figure>
</template>
