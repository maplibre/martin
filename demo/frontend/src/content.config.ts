import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { z } from 'astro/zod';

const featureSchema = z.object({
  body: z.string(),
  icon: z.enum(['Database', 'Layers', 'Zap', 'Users', 'Shield']),
  title: z.string(),
});

const features = defineCollection({
  loader: glob({
    base: './src/content/features',
    pattern: '**/*.yml',
  }),
  schema: featureSchema,
});

const allowedParameterSchema = z.object({
  default: z.union([z.string(), z.number()]).optional(),
  label: z.string().optional(),
  max: z.number().optional(),
  min: z.number().optional(),
  name: z.string(),
  options: z.array(z.string()).optional(),
  type: z.enum(['number', 'string', 'range']),
});

const demoLayerSchema = z.object({
  allowedParameters: z.array(allowedParameterSchema).optional(),
  id: z.string(),
  label: z.string(),
  layerType: z.enum(['fill', 'line']),
  paint: z.record(z.any()),
  sourceLayer: z.string(),
  sqlTemplate: z.string(),
  url: z.string(),
});

const demoLayers = defineCollection({
  loader: glob({
    base: './src/content/demo-layers',
    pattern: '**/*.json',
  }),
  schema: demoLayerSchema,
});

const demoScenarioSchema = z.object({
  id: z.string(),
  label: z.string(),
  layerId: z.string(),
  preset: z.record(z.union([z.string(), z.number()])),
});

const demoScenarios = defineCollection({
  loader: glob({
    base: './src/content/demo-scenarios',
    pattern: '**/*.yml',
  }),
  schema: demoScenarioSchema,
});

export const collections = {
  demoLayers,
  demoScenarios,
  features,
};

export type FeatureEntry = z.infer<typeof featureSchema>;
export type AllowedParameter = z.infer<typeof allowedParameterSchema>;
export type DemoLayerEntry = z.infer<typeof demoLayerSchema>;
export type DemoScenarioEntry = z.infer<typeof demoScenarioSchema>;
