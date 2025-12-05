<template>
  <div class="device-management">
    <div class="header-section">
      <h2>Device Management</h2>
      <button @click="showInviteModal = true" class="btn btn-primary">
        <span class="icon">➕</span> Generate Device Invite
      </button>
    </div>

    <!-- Tabs -->
    <div class="tabs">
      <button
        :class="['tab', { active: activeTab === 'devices' }]"
        @click="activeTab = 'devices'"
      >
        <span class="icon">📱</span> Devices
      </button>
      <button
        :class="['tab', { active: activeTab === 'invites' }]"
        @click="activeTab = 'invites'"
      >
        <span class="icon">🎫</span> Invites
      </button>
    </div>

    <!-- Devices Tab -->
    <div v-if="activeTab === 'devices'" class="devices-tab">
      <div v-if="loading" class="loading">Loading devices...</div>
      <div v-else-if="error" class="error">{{ error }}</div>
      <div v-else>
        <div class="devices-grid">
          <div
            v-for="device in devices"
            :key="device.id"
            class="device-card"
            :class="{ online: device.is_online }"
          >
            <div class="device-header">
              <div class="device-status">
                <span
                  class="status-dot"
                  :class="{ online: device.is_online }"
                ></span>
                <span class="status-text">
                  {{ device.is_online ? "Online" : "Offline" }}
                </span>
              </div>
              <span class="device-kind-badge">{{ device.kind }}</span>
            </div>

            <h3 class="device-name">{{ device.name }}</h3>

            <div class="device-details">
              <div class="detail-row">
                <span class="label">MAC Address:</span>
                <span class="value">{{ device.mac_address || "N/A" }}</span>
              </div>
              <div class="detail-row">
                <span class="label">Platform:</span>
                <span class="value">{{ device.platform || "N/A" }}</span>
              </div>
              <div class="detail-row">
                <span class="label">Version:</span>
                <span class="value">{{
                  device.software_version || "N/A"
                }}</span>
              </div>
              <div class="detail-row">
                <span class="label">IPv4:</span>
                <span class="value">{{ device.ipv4_address || "N/A" }}</span>
              </div>
              <div class="detail-row">
                <span class="label">Uptime:</span>
                <span class="value">{{ formatUptime(device.uptime) }}</span>
              </div>
              <div class="detail-row">
                <span class="label">Last Seen:</span>
                <span class="value">{{
                  formatLastSeen(device.last_seen_at)
                }}</span>
              </div>
            </div>

            <div class="device-actions">
              <button
                @click="renameDevice(device)"
                class="btn btn-small btn-secondary"
              >
                ✏️ Rename
              </button>
              <button
                @click="confirmDelete(device)"
                class="btn btn-small btn-danger"
              >
                🗑️ Delete
              </button>
            </div>
          </div>
        </div>

        <div v-if="devices.length === 0" class="empty-state">
          <div class="empty-icon">📱</div>
          <h3>No devices registered</h3>
          <p>Generate an invite code to register your first device</p>
        </div>
      </div>
    </div>

    <!-- Invites Tab -->
    <div v-if="activeTab === 'invites'" class="invites-tab">
      <div v-if="loadingInvites" class="loading">Loading invites...</div>
      <div v-else-if="invitesError" class="error">{{ invitesError }}</div>
      <div v-else>
        <table class="invites-table">
          <thead>
            <tr>
              <th>Code</th>
              <th>Status</th>
              <th>Expires</th>
              <th>Used By</th>
              <th>Actions</th>
            </tr>
          </thead>
          <tbody>
            <tr v-for="invite in invites" :key="invite.device_code">
              <td>
                <code class="device-code">{{ invite.device_code }}</code>
              </td>
              <td>
                <span class="status-badge" :class="getInviteStatus(invite)">
                  {{ getInviteStatus(invite) }}
                </span>
              </td>
              <td>{{ formatExpiry(invite.expires_at) }}</td>
              <td>
                {{ invite.used_by_device_name || "-" }}
              </td>
              <td>
                <button
                  v-if="!invite.used_at && !isExpired(invite.expires_at)"
                  @click="expireInvite(invite.device_code)"
                  class="btn btn-small btn-danger"
                >
                  Expire
                </button>
              </td>
            </tr>
          </tbody>
        </table>

        <div v-if="invites.length === 0" class="empty-state">
          <div class="empty-icon">🎫</div>
          <h3>No invite codes</h3>
          <p>Generate an invite code to get started</p>
        </div>
      </div>
    </div>

    <!-- Invite Generation Modal -->
    <div v-if="showInviteModal" class="modal-overlay" @click="closeInviteModal">
      <div class="modal-content" @click.stop>
        <div class="modal-header">
          <h3>Generate Device Invite</h3>
          <button @click="closeInviteModal" class="close-btn">✕</button>
        </div>

        <div v-if="generatedInvite" class="generated-invite">
          <div class="invite-success">
            <div class="success-icon">✅</div>
            <h4>Invite code generated!</h4>
          </div>

          <div class="invite-code-display">
            <code class="large-code">{{ generatedInvite.device_code }}</code>
          </div>

          <div class="invite-details">
            <p><strong>Expires:</strong> {{ formatExpiry(generatedInvite.expires_at) }}</p>
            <p class="help-text">
              Use this code with the edge device registration command or web UI
            </p>
          </div>

          <div class="modal-actions">
            <button @click="copyInviteCode" class="btn btn-secondary">
              📋 Copy Code
            </button>
            <button @click="closeInviteModal" class="btn btn-primary">
              Done
            </button>
          </div>
        </div>

        <div v-else>
          <div class="modal-body">
            <p>Generate a new device invite code for registering an edge device.</p>
            <p class="help-text">The code will be valid for 24 hours.</p>
          </div>

          <div class="modal-actions">
            <button @click="closeInviteModal" class="btn btn-secondary">
              Cancel
            </button>
            <button
              @click="generateInvite"
              :disabled="generating"
              class="btn btn-primary"
            >
              {{ generating ? "Generating..." : "Generate Invite" }}
            </button>
          </div>
        </div>
      </div>
    </div>

    <!-- Rename Modal -->
    <div v-if="renameModal.show" class="modal-overlay" @click="closeRenameModal">
      <div class="modal-content" @click.stop>
        <div class="modal-header">
          <h3>Rename Device</h3>
          <button @click="closeRenameModal" class="close-btn">✕</button>
        </div>

        <div class="modal-body">
          <label for="device-name">Device Name</label>
          <input
            id="device-name"
            v-model="renameModal.newName"
            type="text"
            class="form-input"
            placeholder="Enter new device name"
            @keyup.enter="submitRename"
          />
        </div>

        <div class="modal-actions">
          <button @click="closeRenameModal" class="btn btn-secondary">
            Cancel
          </button>
          <button
            @click="submitRename"
            :disabled="!renameModal.newName || renaming"
            class="btn btn-primary"
          >
            {{ renaming ? "Renaming..." : "Rename" }}
          </button>
        </div>
      </div>
    </div>

    <!-- Delete Confirmation Modal -->
    <div v-if="deleteModal.show" class="modal-overlay" @click="closeDeleteModal">
      <div class="modal-content" @click.stop>
        <div class="modal-header">
          <h3>Delete Device</h3>
          <button @click="closeDeleteModal" class="close-btn">✕</button>
        </div>

        <div class="modal-body">
          <div class="warning-box">
            <span class="warning-icon">⚠️</span>
            <div>
              <p>
                <strong>Are you sure you want to delete "{{
                  deleteModal.device?.name
                }}"?</strong>
              </p>
              <p>
                The device will be disconnected and will need to be re-registered to
                reconnect.
              </p>
            </div>
          </div>
        </div>

        <div class="modal-actions">
          <button @click="closeDeleteModal" class="btn btn-secondary">
            Cancel
          </button>
          <button
            @click="submitDelete"
            :disabled="deleting"
            class="btn btn-danger"
          >
            {{ deleting ? "Deleting..." : "Delete Device" }}
          </button>
        </div>
      </div>
    </div>
  </div>
</template>

<script>
import { ref, onMounted } from "vue";
import { apiClient } from "@/utils/api";

export default {
  name: "DeviceManagement",
  setup() {
    const activeTab = ref("devices");
    const loading = ref(false);
    const error = ref(null);
    const devices = ref([]);

    const loadingInvites = ref(false);
    const invitesError = ref(null);
    const invites = ref([]);

    const showInviteModal = ref(false);
    const generating = ref(false);
    const generatedInvite = ref(null);

    const renameModal = ref({
      show: false,
      device: null,
      newName: "",
    });
    const renaming = ref(false);

    const deleteModal = ref({
      show: false,
      device: null,
    });
    const deleting = ref(false);

    const loadDevices = async () => {
      loading.value = true;
      error.value = null;
      try {
        const response = await apiClient.raw.get("/admin/devices");
        devices.value = response.data.data || [];
      } catch (err) {
        error.value = "Failed to load devices: " + err.message;
      } finally {
        loading.value = false;
      }
    };

    const loadInvites = async () => {
      loadingInvites.value = true;
      invitesError.value = null;
      try {
        const response = await apiClient.raw.get("/admin/devices/invites");
        invites.value = response.data.data || [];
      } catch (err) {
        invitesError.value = "Failed to load invites: " + err.message;
      } finally {
        loadingInvites.value = false;
      }
    };

    const generateInvite = async () => {
      generating.value = true;
      try {
        const response = await apiClient.raw.post("/admin/devices/invite", {});
        generatedInvite.value = response.data.data;
        loadInvites(); // Refresh invites list
      } catch (err) {
        alert("Failed to generate invite: " + err.message);
      } finally {
        generating.value = false;
      }
    };

    const copyInviteCode = () => {
      navigator.clipboard.writeText(generatedInvite.value.device_code);
      alert("Invite code copied to clipboard!");
    };

    const closeInviteModal = () => {
      showInviteModal.value = false;
      generatedInvite.value = null;
    };

    const expireInvite = async (code) => {
      if (!confirm("Are you sure you want to expire this invite code?")) return;

      try {
        await apiClient.raw.delete(`/admin/devices/invites/${code}`);
        loadInvites(); // Refresh list
      } catch (err) {
        alert("Failed to expire invite: " + err.message);
      }
    };

    const renameDevice = (device) => {
      renameModal.value = {
        show: true,
        device,
        newName: device.name,
      };
    };

    const closeRenameModal = () => {
      renameModal.value = {
        show: false,
        device: null,
        newName: "",
      };
    };

    const submitRename = async () => {
      renaming.value = true;
      try {
        await apiClient.raw.patch(
          `/admin/devices/${renameModal.value.device.id}/name`,
          { name: renameModal.value.newName }
        );
        loadDevices(); // Refresh devices
        closeRenameModal();
      } catch (err) {
        alert("Failed to rename device: " + err.message);
      } finally {
        renaming.value = false;
      }
    };

    const confirmDelete = (device) => {
      deleteModal.value = {
        show: true,
        device,
      };
    };

    const closeDeleteModal = () => {
      deleteModal.value = {
        show: false,
        device: null,
      };
    };

    const submitDelete = async () => {
      deleting.value = true;
      try {
        await apiClient.raw.delete(`/admin/devices/${deleteModal.value.device.id}`);
        loadDevices(); // Refresh devices
        closeDeleteModal();
      } catch (err) {
        alert("Failed to delete device: " + err.message);
      } finally {
        deleting.value = false;
      }
    };

    const formatUptime = (seconds) => {
      if (!seconds) return "N/A";
      const days = Math.floor(seconds / 86400);
      const hours = Math.floor((seconds % 86400) / 3600);
      const minutes = Math.floor((seconds % 3600) / 60);

      if (days > 0) return `${days}d ${hours}h`;
      if (hours > 0) return `${hours}h ${minutes}m`;
      return `${minutes}m`;
    };

    const formatLastSeen = (timestamp) => {
      if (!timestamp) return "Never";
      const date = new Date(timestamp);
      const now = new Date();
      const diffMs = now - date;
      const diffMins = Math.floor(diffMs / 60000);

      if (diffMins < 1) return "Just now";
      if (diffMins < 60) return `${diffMins} minutes ago`;
      const diffHours = Math.floor(diffMins / 60);
      if (diffHours < 24) return `${diffHours} hours ago`;
      const diffDays = Math.floor(diffHours / 24);
      return `${diffDays} days ago`;
    };

    const formatExpiry = (timestamp) => {
      const date = new Date(timestamp);
      return date.toLocaleString();
    };

    const isExpired = (timestamp) => {
      return new Date(timestamp) < new Date();
    };

    const getInviteStatus = (invite) => {
      if (invite.used_at) return "used";
      if (isExpired(invite.expires_at)) return "expired";
      return "active";
    };

    onMounted(() => {
      loadDevices();
      loadInvites();
    });

    return {
      activeTab,
      loading,
      error,
      devices,
      loadingInvites,
      invitesError,
      invites,
      showInviteModal,
      generating,
      generatedInvite,
      renameModal,
      renaming,
      deleteModal,
      deleting,
      generateInvite,
      copyInviteCode,
      closeInviteModal,
      expireInvite,
      renameDevice,
      closeRenameModal,
      submitRename,
      confirmDelete,
      closeDeleteModal,
      submitDelete,
      formatUptime,
      formatLastSeen,
      formatExpiry,
      isExpired,
      getInviteStatus,
    };
  },
};
</script>

<style scoped>
.device-management {
  padding: 20px;
  max-width: 1400px;
  margin: 0 auto;
}

.header-section {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 30px;
}

h2 {
  margin: 0;
  font-size: 28px;
}

.tabs {
  display: flex;
  gap: 10px;
  margin-bottom: 30px;
  border-bottom: 2px solid #e0e0e0;
}

.tab {
  padding: 12px 24px;
  background: none;
  border: none;
  border-bottom: 3px solid transparent;
  cursor: pointer;
  font-size: 16px;
  color: #666;
  transition: all 0.2s;
}

.tab:hover {
  color: #333;
}

.tab.active {
  color: #2196f3;
  border-bottom-color: #2196f3;
}

.icon {
  margin-right: 8px;
}

.btn {
  padding: 10px 20px;
  border: none;
  border-radius: 6px;
  cursor: pointer;
  font-size: 14px;
  font-weight: 500;
  transition: all 0.2s;
}

.btn-primary {
  background: #2196f3;
  color: white;
}

.btn-primary:hover:not(:disabled) {
  background: #1976d2;
}

.btn-secondary {
  background: #757575;
  color: white;
}

.btn-secondary:hover:not(:disabled) {
  background: #616161;
}

.btn-danger {
  background: #f44336;
  color: white;
}

.btn-danger:hover:not(:disabled) {
  background: #d32f2f;
}

.btn-small {
  padding: 6px 12px;
  font-size: 13px;
}

.btn:disabled {
  opacity: 0.6;
  cursor: not-allowed;
}

.loading,
.error {
  text-align: center;
  padding: 40px;
  font-size: 16px;
}

.error {
  color: #f44336;
}

.devices-grid {
  display: grid;
  grid-template-columns: repeat(auto-fill, minmax(320px, 1fr));
  gap: 20px;
}

.device-card {
  background: white;
  border: 2px solid #e0e0e0;
  border-radius: 8px;
  padding: 20px;
  transition: all 0.2s;
}

.device-card.online {
  border-color: #4caf50;
}

.device-card:hover {
  box-shadow: 0 4px 12px rgba(0, 0, 0, 0.1);
}

.device-header {
  display: flex;
  justify-content: space-between;
  align-items: center;
  margin-bottom: 12px;
}

.device-status {
  display: flex;
  align-items: center;
  gap: 6px;
}

.status-dot {
  width: 10px;
  height: 10px;
  border-radius: 50%;
  background: #9e9e9e;
}

.status-dot.online {
  background: #4caf50;
  animation: pulse 2s infinite;
}

@keyframes pulse {
  0%,
  100% {
    opacity: 1;
  }
  50% {
    opacity: 0.5;
  }
}

.status-text {
  font-size: 13px;
  color: #666;
}

.device-kind-badge {
  background: #e3f2fd;
  color: #1976d2;
  padding: 4px 8px;
  border-radius: 4px;
  font-size: 12px;
  font-weight: 500;
}

.device-name {
  margin: 0 0 16px;
  font-size: 20px;
  color: #333;
}

.device-details {
  margin-bottom: 16px;
}

.detail-row {
  display: flex;
  justify-content: space-between;
  padding: 6px 0;
  font-size: 14px;
}

.detail-row .label {
  color: #666;
}

.detail-row .value {
  color: #333;
  font-weight: 500;
}

.device-actions {
  display: flex;
  gap: 8px;
}

.invites-table {
  width: 100%;
  border-collapse: collapse;
  background: white;
}

.invites-table th,
.invites-table td {
  padding: 12px;
  text-align: left;
  border-bottom: 1px solid #e0e0e0;
}

.invites-table th {
  background: #f5f5f5;
  font-weight: 600;
  color: #666;
}

.device-code {
  background: #f5f5f5;
  padding: 4px 8px;
  border-radius: 4px;
  font-family: monospace;
  font-size: 14px;
}

.large-code {
  font-size: 24px;
  padding: 16px;
}

.status-badge {
  padding: 4px 12px;
  border-radius: 12px;
  font-size: 12px;
  font-weight: 500;
}

.status-badge.active {
  background: #e8f5e9;
  color: #2e7d32;
}

.status-badge.used {
  background: #e3f2fd;
  color: #1565c0;
}

.status-badge.expired {
  background: #ffebee;
  color: #c62828;
}

.empty-state {
  text-align: center;
  padding: 60px 20px;
  color: #666;
}

.empty-icon {
  font-size: 64px;
  margin-bottom: 16px;
}

.modal-overlay {
  position: fixed;
  top: 0;
  left: 0;
  right: 0;
  bottom: 0;
  background: rgba(0, 0, 0, 0.5);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 1000;
}

.modal-content {
  background: white;
  border-radius: 8px;
  width: 90%;
  max-width: 500px;
  max-height: 90vh;
  overflow-y: auto;
}

.modal-header {
  padding: 20px;
  border-bottom: 1px solid #e0e0e0;
  display: flex;
  justify-content: space-between;
  align-items: center;
}

.modal-header h3 {
  margin: 0;
  font-size: 20px;
}

.close-btn {
  background: none;
  border: none;
  font-size: 24px;
  cursor: pointer;
  color: #666;
  padding: 0;
  width: 32px;
  height: 32px;
  display: flex;
  align-items: center;
  justify-content: center;
  border-radius: 4px;
}

.close-btn:hover {
  background: #f5f5f5;
}

.modal-body {
  padding: 20px;
}

.modal-actions {
  padding: 20px;
  border-top: 1px solid #e0e0e0;
  display: flex;
  gap: 10px;
  justify-content: flex-end;
}

.form-input {
  width: 100%;
  padding: 10px;
  border: 1px solid #e0e0e0;
  border-radius: 4px;
  font-size: 14px;
  margin-top: 8px;
}

.form-input:focus {
  outline: none;
  border-color: #2196f3;
}

.generated-invite {
  padding: 20px;
  text-align: center;
}

.invite-success {
  margin-bottom: 24px;
}

.success-icon {
  font-size: 48px;
  margin-bottom: 12px;
}

.invite-success h4 {
  margin: 0;
  font-size: 20px;
  color: #4caf50;
}

.invite-code-display {
  background: #f5f5f5;
  border-radius: 8px;
  padding: 20px;
  margin: 20px 0;
}

.invite-details {
  margin: 20px 0;
}

.help-text {
  color: #666;
  font-size: 14px;
  margin-top: 8px;
}

.warning-box {
  display: flex;
  gap: 12px;
  padding: 16px;
  background: #fff3e0;
  border-left: 4px solid #ff9800;
  border-radius: 4px;
}

.warning-icon {
  font-size: 24px;
}

.warning-box p {
  margin: 4px 0;
}
</style>
