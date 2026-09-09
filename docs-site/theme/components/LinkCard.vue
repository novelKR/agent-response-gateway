<script setup lang="ts">
import { withBase } from 'vitepress';
defineProps<{ title: string; description: string; href: string }>();
</script>
<template>
  <a class="ds-card" :href="withBase(href)">
    <span class="ds-card-title">{{ title }}<span class="ds-card-arrow" aria-hidden="true">↗</span></span>
    <span class="ds-card-description">{{ description }}</span>
  </a>
</template>
