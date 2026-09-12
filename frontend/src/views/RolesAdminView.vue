<template>
  <div class="container mx-auto px-4 py-8">
    <div class="breadcrumbs text-sm mb-6">
      <ul>
        <li><router-link to="/" class="link">Home</router-link></li>
        <li><router-link to="/admin" class="link">Admin</router-link></li>
        <li>Roles &amp; Permissions</li>
      </ul>
    </div>

    <div class="mb-8">
      <h1 class="text-3xl font-bold mb-2">Roles &amp; Permissions</h1>
      <p class="text-base-content/70">
        Define roles, grant them permissions, arrange inheritance, and control who can do what.
        System roles are seeded and protected; custom roles are yours to shape.
      </p>
    </div>

    <div v-if="!canAccessAdmin" class="alert alert-error">
      <span>You need administrator privileges to manage roles.</span>
    </div>

    <template v-else>
      <!-- Feedback -->
      <div v-if="message" class="alert mb-6" :class="messageOk ? 'alert-success' : 'alert-error'">
        <span>{{ message }}</span>
        <button class="btn btn-ghost btn-xs" @click="message = ''">Dismiss</button>
      </div>

      <!-- Loading -->
      <div v-if="isLoading" class="flex justify-center py-12">
        <span class="loading loading-spinner loading-lg"></span>
      </div>

      <template v-else>
        <!-- Create role -->
        <div class="card bg-base-100 shadow-xl mb-8">
          <div class="card-body">
            <h2 class="card-title">Create a role</h2>
            <form class="flex flex-col sm:flex-row gap-3 sm:items-end" @submit.prevent="createRole">
              <div class="form-control">
                <label class="label"><span class="label-text">Name</span></label>
                <input
                  v-model="newRole.name"
                  type="text"
                  class="input input-bordered"
                  placeholder="e.g. shopkeeper"
                  required
                />
              </div>
              <div class="form-control grow">
                <label class="label"><span class="label-text">Description</span></label>
                <input
                  v-model="newRole.description"
                  type="text"
                  class="input input-bordered w-full"
                  placeholder="What this role is for"
                />
              </div>
              <div class="form-control">
                <label class="label"><span class="label-text">Level</span></label>
                <input
                  v-model.number="newRole.level"
                  type="number"
                  min="0"
                  class="input input-bordered w-24"
                />
              </div>
              <button
                class="btn btn-primary"
                :disabled="busy || !newRole.name.trim()"
                type="submit"
              >
                Create
              </button>
            </form>
          </div>
        </div>

        <!-- Roles -->
        <div class="card bg-base-100 shadow-xl mb-8">
          <div class="card-body">
            <h2 class="card-title">Roles</h2>
            <div class="overflow-x-auto">
              <table class="table">
                <thead>
                  <tr>
                    <th>Name</th>
                    <th>Description</th>
                    <th>Level</th>
                    <th>Inherits</th>
                    <th class="text-center">Actions</th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="role in roles" :key="role.id">
                    <td>
                      <span class="font-semibold">{{ role.name }}</span>
                      <span v-if="role.is_system" class="badge badge-ghost badge-sm ml-2"
                        >system</span
                      >
                    </td>
                    <td class="text-sm">{{ role.description }}</td>
                    <td>{{ role.level }}</td>
                    <td class="text-sm">
                      <span v-if="role.inherits.length === 0" class="opacity-50">—</span>
                      <span v-else>{{ role.inherits.map(roleName).join(', ') }}</span>
                    </td>
                    <td class="text-center">
                      <button
                        class="btn btn-ghost btn-xs"
                        :disabled="busy"
                        @click="openInheritance(role)"
                      >
                        Inheritance
                      </button>
                      <button
                        v-if="!role.is_system"
                        class="btn btn-ghost btn-xs text-error"
                        :disabled="busy"
                        @click="removeRole(role)"
                      >
                        Delete
                      </button>
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>

        <!-- Inheritance editor (per role) -->
        <div v-if="editingInheritance" class="card bg-base-100 shadow-xl mb-8">
          <div class="card-body">
            <h2 class="card-title">
              Inheritance for <span class="text-primary">{{ editingInheritance.name }}</span>
            </h2>
            <p class="text-sm text-base-content/70">
              A role gains every permission of the roles it inherits from, transitively. A change
              that would form a cycle is refused.
            </p>
            <div class="flex flex-wrap gap-3 mt-2">
              <label
                v-for="candidate in roles.filter((r) => r.id !== editingInheritance!.id)"
                :key="candidate.id"
                class="label cursor-pointer gap-2 border rounded-lg px-3 py-1"
              >
                <input
                  v-model="inheritanceDraft"
                  type="checkbox"
                  class="checkbox checkbox-sm"
                  :value="candidate.id"
                />
                <span class="label-text">{{ candidate.name }}</span>
              </label>
            </div>
            <div class="card-actions justify-end mt-4">
              <button class="btn btn-ghost btn-sm" @click="editingInheritance = null">
                Cancel
              </button>
              <button class="btn btn-primary btn-sm" :disabled="busy" @click="saveInheritance">
                Save inheritance
              </button>
            </div>
          </div>
        </div>

        <!-- Permission matrix -->
        <div class="card bg-base-100 shadow-xl">
          <div class="card-body">
            <h2 class="card-title">Permission matrix</h2>
            <p class="text-sm text-base-content/70">
              A tick grants the permission to the role directly. Inherited permissions are not shown
              here — they follow from the inheritance above.
            </p>
            <div class="overflow-x-auto mt-2">
              <table class="table table-xs">
                <thead>
                  <tr>
                    <th class="sticky left-0 bg-base-100">Permission</th>
                    <th v-for="role in roles" :key="role.id" class="text-center">
                      {{ role.name }}
                    </th>
                  </tr>
                </thead>
                <tbody>
                  <tr v-for="perm in permissions" :key="perm.key">
                    <td class="sticky left-0 bg-base-100">
                      <div class="font-mono text-xs">{{ perm.key }}</div>
                      <div class="opacity-60 text-xs">{{ perm.description }}</div>
                    </td>
                    <td v-for="role in roles" :key="role.id" class="text-center">
                      <input
                        type="checkbox"
                        class="checkbox checkbox-xs"
                        :checked="role.permissions.includes(perm.key)"
                        :disabled="busy"
                        :aria-label="`${perm.key} for ${role.name}`"
                        @change="
                          togglePermission(
                            role,
                            perm.key,
                            ($event.target as HTMLInputElement).checked
                          )
                        "
                      />
                    </td>
                  </tr>
                </tbody>
              </table>
            </div>
          </div>
        </div>
      </template>
    </template>
  </div>
</template>

<script setup lang="ts">
import { computed, onMounted, ref } from 'vue'
import { useAuthStore } from '@/stores/auth'
import { rbacApi } from '@/utils/api'
import { UserRole, type RbacRole, type RbacPermission } from '@/types'

const authStore = useAuthStore()
const canAccessAdmin = computed(() => authStore.isAdmin || authStore.hasRole(UserRole.Admin))

const roles = ref<RbacRole[]>([])
const permissions = ref<RbacPermission[]>([])
const isLoading = ref(false)
const busy = ref(false)
const message = ref('')
const messageOk = ref(false)

const newRole = ref<{ name: string; description: string; level: number }>({
  name: '',
  description: '',
  level: 0,
})

const editingInheritance = ref<RbacRole | null>(null)
const inheritanceDraft = ref<string[]>([])

const roleName = (id: string): string => roles.value.find((r) => r.id === id)?.name ?? id

function say(ok: boolean, text: string) {
  messageOk.value = ok
  message.value = text
}

async function load() {
  isLoading.value = true
  const res = await rbacApi.config()
  if (res.success && res.data) {
    roles.value = [...res.data.roles].sort((a, b) => a.level - b.level)
    permissions.value = [...res.data.permissions].sort((a, b) => a.key.localeCompare(b.key))
  } else {
    say(false, res.error || 'Failed to load the RBAC configuration')
  }
  isLoading.value = false
}

async function createRole() {
  if (!newRole.value.name.trim()) return
  busy.value = true
  const res = await rbacApi.createRole({
    name: newRole.value.name.trim(),
    description: newRole.value.description.trim(),
    level: newRole.value.level,
  })
  if (res.success) {
    say(true, `Created role "${newRole.value.name.trim()}".`)
    newRole.value = { name: '', description: '', level: 0 }
    await load()
  } else {
    say(false, res.error || 'Failed to create the role')
  }
  busy.value = false
}

async function removeRole(role: RbacRole) {
  busy.value = true
  const res = await rbacApi.removeRole(role.id)
  if (res.success) {
    say(true, `Deleted role "${role.name}".`)
    await load()
  } else {
    say(false, res.error || 'Failed to delete the role')
  }
  busy.value = false
}

async function togglePermission(role: RbacRole, key: string, granted: boolean) {
  busy.value = true
  const next = granted
    ? [...new Set([...role.permissions, key])]
    : role.permissions.filter((k) => k !== key)
  const res = await rbacApi.setPermissions(role.id, next)
  if (res.success) {
    role.permissions = next
    say(true, `Updated permissions for "${role.name}".`)
  } else {
    say(false, res.error || 'Failed to update permissions')
    // Reload so the checkbox reflects the server, not the failed intent.
    await load()
  }
  busy.value = false
}

function openInheritance(role: RbacRole) {
  editingInheritance.value = role
  inheritanceDraft.value = [...role.inherits]
}

async function saveInheritance() {
  if (!editingInheritance.value) return
  const role = editingInheritance.value
  busy.value = true
  const res = await rbacApi.setInheritance(role.id, inheritanceDraft.value)
  if (res.success) {
    say(true, `Updated inheritance for "${role.name}".`)
    editingInheritance.value = null
    await load()
  } else {
    // The most likely failure is a cycle; surface the server's message.
    say(false, res.error || 'Failed to update inheritance')
  }
  busy.value = false
}

onMounted(() => {
  if (canAccessAdmin.value) void load()
})
</script>
