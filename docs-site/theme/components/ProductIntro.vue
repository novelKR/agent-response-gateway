<script setup lang="ts">
import { useContent } from '../content';
import StatusBadge from './StatusBadge.vue';
import DiagramFigure from './DiagramFigure.vue';
const { ui } = useContent();
</script>
<template><section class="ds-intro"><div class="ds-copy"><div class="ds-eyebrow"><StatusBadge>{{ ui.stage }}</StatusBadge><span>{{ ui.local }}</span></div><slot /></div><DiagramFigure kind="routes" /></section></template>
