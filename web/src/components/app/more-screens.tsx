/**
 * The screens for evaluation, datasets, themes, metrics and monitors, registered in one
 * list so the selector and the router in `App.tsx` read them the same way.
 */

import type { ComponentType } from 'react'
import Datasets from './Datasets'
import Evaluations from './Evaluations'
import Metrics from './Metrics'
import Monitors from './Monitors'
import Themes from './Themes'
import type { ScreenProps } from './screen-kit'

export type MoreScreenId = 'evaluations' | 'datasets' | 'themes' | 'metrics' | 'monitors'

export const MORE_SCREENS: { id: MoreScreenId; label: string; Screen: ComponentType<ScreenProps> }[] = [
  { id: 'evaluations', label: 'Evaluations', Screen: Evaluations },
  { id: 'datasets', label: 'Datasets', Screen: Datasets },
  { id: 'themes', label: 'Themes', Screen: Themes },
  { id: 'metrics', label: 'Metrics', Screen: Metrics },
  { id: 'monitors', label: 'Monitors', Screen: Monitors },
]
