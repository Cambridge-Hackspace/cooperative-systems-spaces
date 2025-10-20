<template>
  <div class="calendar-events">
    <div class="calendar-header">
      <h2>📅 Upcoming Events</h2>
      <button 
        v-if="!loading" 
        @click="refreshEvents" 
        class="refresh-btn"
        :disabled="refreshing"
        title="Refresh calendar events"
      >
        <span v-if="refreshing">🔄</span>
        <span v-else>↻</span>
      </button>
    </div>

    <div v-if="loading" class="loading-state">
      <div class="spinner"></div>
      <p>Loading events...</p>
    </div>

    <div v-else-if="error" class="error-state">
      <p>{{ error }}</p>
      <button @click="fetchEvents" class="retry-btn">Try Again</button>
    </div>

    <div v-else-if="events.length === 0" class="empty-state">
      <p>No upcoming events scheduled</p>
    </div>

    <div v-else class="events-list">
      <div 
        v-for="event in events" 
        :key="event.title + event.start"
        class="event-card"
        :style="{ borderLeftColor: event.calendar_color }"
      >
        <div class="event-date">
          <div class="event-day">{{ formatDay(event.start) }}</div>
          <div class="event-month">{{ formatMonth(event.start) }}</div>
        </div>

        <div class="event-details">
          <h3 class="event-title">{{ event.title }}</h3>
          
          <div class="event-meta">
            <span class="event-time">
              <span v-if="event.all_day">
                🕐 All Day
              </span>
              <span v-else>
                🕐 {{ formatTime(event.start) }}
                <span v-if="event.end"> - {{ formatTime(event.end) }}</span>
              </span>
            </span>

            <span 
              v-if="event.location" 
              class="event-location"
              :title="event.location"
            >
              📍 {{ event.location }}
            </span>
          </div>

          <p v-if="event.description" class="event-description" v-html="event.description">
          </p>

          <div class="event-calendar-tag" :style="{ backgroundColor: event.calendar_color }">
            {{ event.calendar_name }}
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<script>
export default {
  name: 'CalendarEvents',
  
  data() {
    return {
      events: [],
      loading: true,
      refreshing: false,
      error: null,
      refreshInterval: null,
    };
  },

  mounted() {
    this.fetchEvents();
    // Auto-refresh every 15 minutes
    this.refreshInterval = setInterval(() => {
      this.fetchEvents(true); // Silent refresh
    }, 15 * 60 * 1000);
  },

  beforeUnmount() {
    if (this.refreshInterval) {
      clearInterval(this.refreshInterval);
    }
  },

  methods: {
    async fetchEvents(silent = false) {
      if (!silent) {
        this.loading = true;
      }
      this.error = null;

      try {
        const response = await fetch('/api/calendar/events');
        
        if (!response.ok) {
          throw new Error(`Failed to fetch events: ${response.statusText}`);
        }

        this.events = await response.json();
      } catch (err) {
        console.error('Error fetching calendar events:', err);
        this.error = err.message || 'Failed to load calendar events';
      } finally {
        this.loading = false;
      }
    },

    async refreshEvents() {
      this.refreshing = true;
      this.error = null;

      try {
        const response = await fetch('/api/calendar/events/refresh');
        
        if (!response.ok) {
          throw new Error(`Failed to refresh events: ${response.statusText}`);
        }

        this.events = await response.json();
      } catch (err) {
        console.error('Error refreshing calendar events:', err);
        this.error = err.message || 'Failed to refresh calendar events';
      } finally {
        this.refreshing = false;
      }
    },

    formatDay(dateString) {
      const date = new Date(dateString);
      return date.getDate();
    },

    formatMonth(dateString) {
      const date = new Date(dateString);
      return date.toLocaleDateString('en-US', { month: 'short' });
    },

    formatTime(dateString) {
      const date = new Date(dateString);
      return date.toLocaleTimeString('en-US', { 
        hour: 'numeric', 
        minute: '2-digit',
        hour12: true 
      });
    },
  },
};
</script>

<style scoped>
.calendar-events {
  background: white;
  border-radius: 8px;
  padding: 1.5rem;
  box-shadow: 0 2px 4px rgba(0, 0, 0, 0.1);
}

.calendar-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 1.5rem;
}

.calendar-header h2 {
  margin: 0;
  font-size: 1.5rem;
  color: #333;
}

.refresh-btn {
  background: none;
  border: 1px solid #ddd;
  border-radius: 4px;
  padding: 0.5rem 0.75rem;
  cursor: pointer;
  font-size: 1.2rem;
  transition: all 0.2s;
}

.refresh-btn:hover:not(:disabled) {
  background: #f5f5f5;
  border-color: #999;
}

.refresh-btn:disabled {
  opacity: 0.5;
  cursor: not-allowed;
}

.loading-state,
.error-state,
.empty-state {
  text-align: center;
  padding: 2rem;
  color: #666;
}

.spinner {
  border: 3px solid #f3f3f3;
  border-top: 3px solid #3788d8;
  border-radius: 50%;
  width: 40px;
  height: 40px;
  animation: spin 1s linear infinite;
  margin: 0 auto 1rem;
}

@keyframes spin {
  0% { transform: rotate(0deg); }
  100% { transform: rotate(360deg); }
}

.error-state {
  color: #d9534f;
}

.retry-btn {
  margin-top: 1rem;
  padding: 0.5rem 1rem;
  background: #3788d8;
  color: white;
  border: none;
  border-radius: 4px;
  cursor: pointer;
  transition: background 0.2s;
}

.retry-btn:hover {
  background: #2a6ab8;
}

.events-list {
  display: flex;
  flex-direction: column;
  gap: 1rem;
}

.event-card {
  display: flex;
  gap: 1rem;
  padding: 1rem;
  border: 1px solid #e0e0e0;
  border-left: 4px solid;
  border-radius: 6px;
  transition: all 0.2s;
  background: #fafafa;
}

.event-card:hover {
  box-shadow: 0 2px 8px rgba(0, 0, 0, 0.1);
  transform: translateY(-2px);
}

.event-date {
  display: flex;
  flex-direction: column;
  align-items: center;
  justify-content: center;
  min-width: 60px;
  padding: 0.5rem;
  background: white;
  border-radius: 4px;
  border: 1px solid #e0e0e0;
}

.event-day {
  font-size: 1.75rem;
  font-weight: bold;
  line-height: 1;
  color: #333;
}

.event-month {
  font-size: 0.875rem;
  text-transform: uppercase;
  color: #666;
  margin-top: 0.25rem;
}

.event-details {
  flex: 1;
  min-width: 0;
}

.event-title {
  margin: 0 0 0.5rem 0;
  font-size: 1.125rem;
  color: #333;
  font-weight: 600;
}

.event-meta {
  display: flex;
  flex-wrap: wrap;
  gap: 1rem;
  margin-bottom: 0.5rem;
  font-size: 0.875rem;
  color: #666;
}

.event-time,
.event-location {
  display: flex;
  align-items: center;
  gap: 0.25rem;
}

.event-location {
  max-width: 200px;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.event-description {
  margin: 0.5rem 0;
  font-size: 0.875rem;
  color: #555;
  line-height: 1.4;
}

.event-calendar-tag {
  display: inline-block;
  padding: 0.25rem 0.75rem;
  border-radius: 12px;
  font-size: 0.75rem;
  color: white;
  font-weight: 500;
  margin-top: 0.5rem;
}

/* Responsive design */
@media (max-width: 640px) {
  .event-card {
    flex-direction: column;
  }

  .event-date {
    flex-direction: row;
    justify-content: flex-start;
    width: 100%;
    gap: 0.5rem;
  }

  .event-meta {
    flex-direction: column;
    gap: 0.5rem;
  }

  .event-location {
    max-width: 100%;
  }
}
</style>
