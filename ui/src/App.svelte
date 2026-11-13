<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { onMount } from "svelte";

  let json = $state("loading...");

  async function refresh() {
    try {
      const projects = await invoke("list_projects");
      console.log("list_projects", projects);
      json = JSON.stringify(projects, null, 2);
    } catch (e) {
      json = `error: ${e}`;
    }
  }

  onMount(() => {
    refresh();
    const un = listen("claudegate://changed", refresh);
    return () => {
      un.then((f) => f());
    };
  });
</script>

<main>
  <h1>ClaudeGate</h1>
  <pre>{json}</pre>
</main>
