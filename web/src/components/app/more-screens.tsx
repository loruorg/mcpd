/**
 * The screens for evaluation, datasets, metrics and monitors, registered in one
 * list so the selector and the router in `App.tsx` read them the same way.
 */

import type { ComponentType } from 'react'
import Datasets from './Datasets'
import Evaluations from './Evaluations'
import Metrics from './Metrics'
import Monitors from './Monitors'
import Prompts from './Prompts'
import type { ScreenProps } from './screen-kit'

export type MoreScreenId = 'prompts' | 'evaluations' | 'datasets' | 'metrics' | 'monitors'

export const MORE_SCREENS: { id: MoreScreenId; label: string; Screen: ComponentType<ScreenProps> }[] = [
  { id: 'prompts', label: 'Prompts', Screen: Prompts },
  { id: 'evaluations', label: 'Evaluations', Screen: Evaluations },
  { id: 'datasets', label: 'Datasets', Screen: Datasets },
  { id: 'metrics', label: 'Metrics', Screen: Metrics },
  { id: 'monitors', label: 'Monitors', Screen: Monitors },
]
