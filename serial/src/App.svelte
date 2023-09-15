<script>
  import SerialPortsList from "./lib/SerialPortsList.svelte";
</script>

{#if "serial" in navigator}
  <SerialPortsList />
{:else}
  <p>Your Browser is not supported</p>
  <p>
    This app requires the Web Serial API to interact with serial devices which
    not supported by your Browser.
    <a href="https://caniuse.com/web-serial"
      >You can check if your browser is supported here.</a
    >
  </p>
  <p>As of writing Chrome, Edge and Opera support the API.</p>
{/if}
