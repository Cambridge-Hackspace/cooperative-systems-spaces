import type { DayOfWeek, ScheduleInterval } from '@/types'

/** Days in iteration order so the editor renders Mon → Sun. */
export const ALL_DAYS: DayOfWeek[] = ['mon', 'tue', 'wed', 'thu', 'fri', 'sat', 'sun']
export const WEEKDAYS: DayOfWeek[] = ['mon', 'tue', 'wed', 'thu', 'fri']
export const WEEKEND: DayOfWeek[] = ['sat', 'sun']

/** Helpers used by template builders. */
function window(days: DayOfWeek[], start: string, end: string): ScheduleInterval[] {
  return days.map(day => ({ day, start, end }))
}
function allDay(days: DayOfWeek[]): ScheduleInterval[] {
  return window(days, '00:00', '23:59')
}

export interface ScheduleTemplate {
  id: string
  label: string
  description?: string
  build: () => ScheduleInterval[]
}

/** Two-click presets so creating a "Mon–Fri 9-5" schedule isn't a 14-input chore. */
export const SCHEDULE_TEMPLATES: ScheduleTemplate[] = [
  {
    id: '24-7',
    label: '24 / 7',
    description: 'Always open',
    build: () => allDay(ALL_DAYS),
  },
  {
    id: 'weekday-9-5',
    label: '9–5 weekdays',
    description: 'Mon–Fri 09:00–17:00',
    build: () => window(WEEKDAYS, '09:00', '17:00'),
  },
  {
    id: 'business-9-6',
    label: 'Business hours',
    description: 'Mon–Fri 09:00–18:00',
    build: () => window(WEEKDAYS, '09:00', '18:00'),
  },
  {
    id: 'weekday-eve',
    label: 'Weekday evenings',
    description: 'Mon–Fri 17:00–22:00',
    build: () => window(WEEKDAYS, '17:00', '22:00'),
  },
  {
    id: 'weekends',
    label: 'Weekends only',
    description: 'Sat–Sun all day',
    build: () => allDay(WEEKEND),
  },
]
