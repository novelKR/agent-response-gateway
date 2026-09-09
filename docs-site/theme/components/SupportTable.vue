<script setup lang="ts">
import { useContent } from '../content';
defineProps<{ label?: string }>();
const { ui } = useContent();
</script>
<template><div class="ds-support" role="region" :aria-label="label || ui.table" tabindex="0"><slot /></div></template>
