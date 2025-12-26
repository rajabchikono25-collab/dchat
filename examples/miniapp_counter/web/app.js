/**
 * Counter Mini-App with On-Chain Program Integration
 *
 * Demonstrates a mini-app that interacts with a DPL (dchat Program Language) smart contract:
 * - Initializes on-chain counter account
 * - Sends increment/decrement transactions
 * - Displays transaction history and events
 * - Shows compute units consumed
 */

(function () {
  "use strict";

  // ========================================
  // State
  // ========================================

  let state = {
    // Local counter (for offline/fallback)
    localValue: 0,
    // On-chain counter state
    onChainValue: null,
    onChainInitialized: false,
    // Stats
    totalIncrements: 0,
    totalDecrements: 0,
    // Transaction history
    transactions: [],
    // Events from program
    events: [],
    // Current mode: 'local' or 'onchain'
    mode: "local",
    // Last slot
    lastSlot: 0,
  };

  // ========================================
  // DOM Elements
  // ========================================

  const elements = {
    counterValue: document.getElementById("counter-value"),
    totalIncrements: document.getElementById("total-increments"),
    totalDecrements: document.getElementById("total-decrements"),
    btnIncrement: document.getElementById("btn-increment"),
    btnDecrement: document.getElementById("btn-decrement"),
    btnReset: document.getElementById("btn-reset"),
    btnSet: document.getElementById("btn-set"),
    connectionStatus: document.getElementById("connection-status"),
    lastAction: document.getElementById("last-action"),
    modalOverlay: document.getElementById("modal-overlay"),
    setValueInput: document.getElementById("set-value-input"),
    modalCancel: document.getElementById("modal-cancel"),
    modalConfirm: document.getElementById("modal-confirm"),
  };

  // ========================================
  // On-Chain Panel (dynamically created)
  // ========================================

  function createOnChainPanel() {
    // Check if panel already exists
    if (document.getElementById("onchain-panel")) return;

    const panel = document.createElement("div");
    panel.id = "onchain-panel";
    panel.className = "onchain-panel";
    panel.innerHTML = `
      <div class="panel-header">
        <span class="panel-icon">⛓️</span>
        <span class="panel-title">On-Chain State</span>
        <span class="chain-status" id="chain-status">Not initialized</span>
      </div>
      
      <div class="panel-content">
        <div class="onchain-row">
          <span class="label">On-Chain Value:</span>
          <span class="value" id="onchain-value">—</span>
        </div>
        <div class="onchain-row">
          <span class="label">Last Updated Slot:</span>
          <span class="value" id="last-slot">—</span>
        </div>
        <div class="onchain-row">
          <span class="label">Total Operations:</span>
          <span class="value" id="total-ops">0</span>
        </div>
      </div>
      
      <div class="panel-section">
        <div class="section-header">Recent Transactions</div>
        <div class="tx-list" id="tx-list">
          <div class="tx-empty">No transactions yet</div>
        </div>
      </div>
      
      <div class="panel-section">
        <div class="section-header">Program Events</div>
        <div class="event-list" id="event-list">
          <div class="event-empty">No events yet</div>
        </div>
      </div>
      
      <div class="panel-actions">
        <button id="btn-init-onchain" class="btn btn-primary">
          Initialize On-Chain Counter
        </button>
        <button id="btn-sync" class="btn btn-secondary" disabled>
          Sync State
        </button>
      </div>
    `;

    // Add styles
    const style = document.createElement("style");
    style.textContent = `
      .onchain-panel {
        background: var(--surface);
        border: 1px solid var(--surface-light);
        border-radius: 16px;
        margin-top: 1.5rem;
        overflow: hidden;
      }
      
      .panel-header {
        background: linear-gradient(135deg, #6366f1, #8b5cf6);
        padding: 1rem;
        display: flex;
        align-items: center;
        gap: 0.5rem;
      }
      
      .panel-icon {
        font-size: 1.25rem;
      }
      
      .panel-title {
        font-weight: 600;
        flex: 1;
      }
      
      .chain-status {
        font-size: 0.75rem;
        padding: 0.25rem 0.5rem;
        background: rgba(255,255,255,0.2);
        border-radius: 999px;
      }
      
      .chain-status.active {
        background: #22c55e;
      }
      
      .panel-content {
        padding: 1rem;
        border-bottom: 1px solid var(--surface-light);
      }
      
      .onchain-row {
        display: flex;
        justify-content: space-between;
        padding: 0.5rem 0;
      }
      
      .onchain-row .label {
        color: var(--text-secondary);
      }
      
      .onchain-row .value {
        font-family: 'JetBrains Mono', monospace;
        font-weight: 600;
        color: var(--accent);
      }
      
      .panel-section {
        padding: 1rem;
        border-bottom: 1px solid var(--surface-light);
      }
      
      .section-header {
        font-size: 0.75rem;
        text-transform: uppercase;
        color: var(--text-secondary);
        margin-bottom: 0.75rem;
        letter-spacing: 0.5px;
      }
      
      .tx-list, .event-list {
        max-height: 150px;
        overflow-y: auto;
      }
      
      .tx-item {
        display: flex;
        align-items: center;
        gap: 0.5rem;
        padding: 0.5rem;
        background: var(--surface-light);
        border-radius: 8px;
        margin-bottom: 0.5rem;
        font-size: 0.8rem;
      }
      
      .tx-item.success {
        border-left: 3px solid #22c55e;
      }
      
      .tx-item.failed {
        border-left: 3px solid #ef4444;
      }
      
      .tx-sig {
        font-family: 'JetBrains Mono', monospace;
        font-size: 0.7rem;
        color: var(--text-secondary);
        flex: 1;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      
      .tx-type {
        font-weight: 500;
        min-width: 80px;
      }
      
      .event-item {
        padding: 0.5rem;
        background: var(--surface-light);
        border-radius: 8px;
        margin-bottom: 0.5rem;
        font-size: 0.8rem;
      }
      
      .event-type {
        color: #f59e0b;
        font-weight: 600;
        margin-bottom: 0.25rem;
      }
      
      .event-data {
        font-family: 'JetBrains Mono', monospace;
        font-size: 0.7rem;
        color: var(--text-secondary);
      }
      
      .tx-empty, .event-empty {
        color: var(--text-secondary);
        font-style: italic;
        font-size: 0.8rem;
        text-align: center;
        padding: 1rem;
      }
      
      .panel-actions {
        padding: 1rem;
        display: flex;
        gap: 0.5rem;
      }
      
      .panel-actions .btn {
        flex: 1;
        padding: 0.75rem;
        border: none;
        border-radius: 8px;
        font-weight: 600;
        cursor: pointer;
        transition: all 0.2s;
      }
      
      .panel-actions .btn:disabled {
        opacity: 0.5;
        cursor: not-allowed;
      }
      
      .panel-actions .btn-primary {
        background: linear-gradient(135deg, #6366f1, #8b5cf6);
        color: white;
      }
      
      .panel-actions .btn-secondary {
        background: var(--surface-light);
        color: var(--text);
      }
      
      .panel-actions .btn:hover:not(:disabled) {
        transform: translateY(-2px);
      }
      
      /* Compute units badge */
      .compute-badge {
        font-size: 0.65rem;
        background: var(--surface);
        padding: 0.15rem 0.4rem;
        border-radius: 4px;
        color: var(--text-secondary);
      }
    `;
    document.head.appendChild(style);

    // Insert panel after the actions
    const container = document.querySelector(".container");
    container.appendChild(panel);

    // Set up event listeners for new buttons
    document
      .getElementById("btn-init-onchain")
      .addEventListener("click", initializeOnChain);
    document
      .getElementById("btn-sync")
      .addEventListener("click", syncOnChainState);
  }

  // ========================================
  // UI Updates
  // ========================================

  function updateDisplay() {
    // Show on-chain value if initialized, otherwise local
    const displayValue =
      state.onChainInitialized && state.onChainValue !== null
        ? state.onChainValue
        : state.localValue;

    elements.counterValue.textContent = displayValue;
    elements.totalIncrements.textContent = state.totalIncrements;
    elements.totalDecrements.textContent = state.totalDecrements;

    // Pulse animation
    const display = document.querySelector(".counter-display");
    display.classList.remove("pulse");
    void display.offsetWidth; // Force reflow
    display.classList.add("pulse");

    // Update on-chain panel
    updateOnChainPanel();
  }

  function updateOnChainPanel() {
    const onchainValue = document.getElementById("onchain-value");
    const chainStatus = document.getElementById("chain-status");
    const lastSlot = document.getElementById("last-slot");
    const totalOps = document.getElementById("total-ops");
    const btnInit = document.getElementById("btn-init-onchain");
    const btnSync = document.getElementById("btn-sync");

    if (!onchainValue) return;

    if (state.onChainInitialized) {
      onchainValue.textContent = state.onChainValue ?? "—";
      chainStatus.textContent = "Active";
      chainStatus.className = "chain-status active";
      lastSlot.textContent = state.lastSlot || "—";
      btnInit.disabled = true;
      btnInit.textContent = "✓ Initialized";
      btnSync.disabled = false;
    } else {
      onchainValue.textContent = "—";
      chainStatus.textContent = "Not initialized";
      chainStatus.className = "chain-status";
      lastSlot.textContent = "—";
    }

    totalOps.textContent = state.transactions.filter((t) => t.success).length;

    // Update transaction list
    const txList = document.getElementById("tx-list");
    if (txList) {
      if (state.transactions.length === 0) {
        txList.innerHTML = '<div class="tx-empty">No transactions yet</div>';
      } else {
        txList.innerHTML = state.transactions
          .slice(0, 10)
          .map(
            (tx) => `
          <div class="tx-item ${tx.success ? "success" : "failed"}">
            <span class="tx-type">${tx.type || "Unknown"}</span>
            <span class="tx-sig" title="${tx.signature}">${tx.signature.substring(0, 16)}...</span>
            ${tx.computeUnits ? `<span class="compute-badge">${tx.computeUnits} CU</span>` : ""}
          </div>
        `,
          )
          .join("");
      }
    }

    // Update event list
    const eventList = document.getElementById("event-list");
    if (eventList) {
      if (state.events.length === 0) {
        eventList.innerHTML = '<div class="event-empty">No events yet</div>';
      } else {
        eventList.innerHTML = state.events
          .slice(0, 10)
          .map(
            (ev) => `
          <div class="event-item">
            <div class="event-type">${ev.type}</div>
            <div class="event-data">${formatEventData(ev)}</div>
          </div>
        `,
          )
          .join("");
      }
    }
  }

  function formatEventData(event) {
    switch (event.type) {
      case "CounterInitialized":
        return `Initial: ${event.initialValue}`;
      case "CounterChanged":
        return `${event.oldValue} → ${event.newValue}`;
      case "CounterReset":
        return `Reset from ${event.oldValue}`;
      default:
        return JSON.stringify(event).substring(0, 50);
    }
  }

  function setStatus(status, type = "connecting") {
    elements.connectionStatus.textContent = status;
    elements.connectionStatus.className = `status status-${type}`;
  }

  function setLastAction(action) {
    elements.lastAction.textContent = action;
  }

  function showModal() {
    elements.modalOverlay.classList.remove("hidden");
    elements.setValueInput.value = "";
    elements.setValueInput.focus();
  }

  function hideModal() {
    elements.modalOverlay.classList.add("hidden");
  }

  // ========================================
  // On-Chain Program Calls
  // ========================================

  async function callProgram(action, payload = {}) {
    if (!window.dchat) {
      console.debug("dchat not available, using local mode");
      return null;
    }

    return new Promise((resolve, reject) => {
      const requestId = `req_${Date.now()}_${Math.random().toString(36).substr(2, 9)}`;

      const handler = (event) => {
        const msg = event.data;
        if (
          msg.type === "dchat:program:response" &&
          (msg.requestId === requestId || msg.payload?.requestId === requestId)
        ) {
          window.removeEventListener("message", handler);

          if (msg.payload?.success) {
            resolve(msg.payload);
          } else {
            reject(new Error(msg.payload?.error || "Program call failed"));
          }
        }
      };

      window.addEventListener("message", handler);

      // Send program call request
      window.parent.postMessage(
        {
          type: "dchat:program:call",
          payload: {
            requestId,
            action,
            ...payload,
          },
        },
        "*",
      );

      // Timeout
      setTimeout(() => {
        window.removeEventListener("message", handler);
        reject(new Error("Program call timeout"));
      }, 10000);
    });
  }

  async function initializeOnChain() {
    try {
      const btn = document.getElementById("btn-init-onchain");
      btn.disabled = true;
      btn.textContent = "Initializing...";

      const result = await callProgram("initialize", {
        value: state.localValue,
      });

      if (result && result.success) {
        state.onChainInitialized = true;
        state.onChainValue = result.account?.value ?? state.localValue;
        state.lastSlot = result.slot;

        // Record transaction
        state.transactions.unshift({
          signature: result.signature,
          type: "Initialize",
          success: true,
          slot: result.slot,
          computeUnits: result.computeUnits,
        });

        // Record events
        if (result.events) {
          state.events.unshift(...result.events);
        }

        updateDisplay();
        setLastAction(
          `Initialized on-chain counter with value ${state.onChainValue}`,
        );

        if (window.dchat?.ui) {
          window.dchat.ui.haptic("success");
        }
      }
    } catch (e) {
      console.error("Failed to initialize on-chain:", e);
      setLastAction(`Failed: ${e.message}`);

      const btn = document.getElementById("btn-init-onchain");
      btn.disabled = false;
      btn.textContent = "Initialize On-Chain Counter";
    }
  }

  async function syncOnChainState() {
    if (!window.dchat) return;

    try {
      const result = await new Promise((resolve, reject) => {
        const requestId = `sync_${Date.now()}`;

        const handler = (event) => {
          const msg = event.data;
          if (
            msg.type === "dchat:program:state" &&
            (msg.requestId === requestId ||
              msg.payload?.requestId === requestId)
          ) {
            window.removeEventListener("message", handler);
            resolve(msg.payload);
          }
        };

        window.addEventListener("message", handler);

        window.parent.postMessage(
          {
            type: "dchat:program:query",
            payload: { requestId },
          },
          "*",
        );

        setTimeout(() => {
          window.removeEventListener("message", handler);
          reject(new Error("Sync timeout"));
        }, 5000);
      });

      if (result && result.account) {
        state.onChainValue = result.account.value;
        state.lastSlot = result.account.lastUpdated;
        updateDisplay();
        setLastAction("Synced with on-chain state");
      }
    } catch (e) {
      console.error("Failed to sync:", e);
    }
  }

  // ========================================
  // Counter Actions (with on-chain integration)
  // ========================================

  async function increment() {
    try {
      elements.btnIncrement.disabled = true;

      if (state.onChainInitialized) {
        // Call on-chain program
        const result = await callProgram("increment");

        if (result && result.success) {
          state.onChainValue = result.account?.value ?? state.onChainValue + 1;
          state.lastSlot = result.slot;

          state.transactions.unshift({
            signature: result.signature,
            type: "Increment",
            success: true,
            slot: result.slot,
            computeUnits: result.computeUnits,
          });

          if (result.events) {
            state.events.unshift(...result.events);
          }

          state.totalIncrements++;
          updateDisplay();
          saveState();
          setLastAction(
            `On-chain increment to ${state.onChainValue} (slot ${result.slot})`,
          );
        }
      } else {
        // Local mode
        state.localValue++;
        state.totalIncrements++;
        updateDisplay();
        saveState();
        setLastAction(`Incremented to ${state.localValue} (local)`);
      }

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("light");
      }
    } catch (e) {
      console.error("Increment failed:", e);

      // Record failed transaction
      state.transactions.unshift({
        signature: `failed_${Date.now()}`,
        type: "Increment",
        success: false,
        error: e.message,
      });

      setLastAction(`Failed: ${e.message}`);
      updateDisplay();
    } finally {
      elements.btnIncrement.disabled = false;
    }
  }

  async function decrement() {
    try {
      elements.btnDecrement.disabled = true;

      if (state.onChainInitialized) {
        const result = await callProgram("decrement");

        if (result && result.success) {
          state.onChainValue = result.account?.value ?? state.onChainValue - 1;
          state.lastSlot = result.slot;

          state.transactions.unshift({
            signature: result.signature,
            type: "Decrement",
            success: true,
            slot: result.slot,
            computeUnits: result.computeUnits,
          });

          if (result.events) {
            state.events.unshift(...result.events);
          }

          state.totalDecrements++;
          updateDisplay();
          saveState();
          setLastAction(
            `On-chain decrement to ${state.onChainValue} (slot ${result.slot})`,
          );
        }
      } else {
        state.localValue--;
        state.totalDecrements++;
        updateDisplay();
        saveState();
        setLastAction(`Decremented to ${state.localValue} (local)`);
      }

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("light");
      }
    } catch (e) {
      console.error("Decrement failed:", e);

      state.transactions.unshift({
        signature: `failed_${Date.now()}`,
        type: "Decrement",
        success: false,
        error: e.message,
      });

      setLastAction(`Failed: ${e.message}`);
      updateDisplay();
    } finally {
      elements.btnDecrement.disabled = false;
    }
  }

  async function reset() {
    try {
      elements.btnReset.disabled = true;

      if (state.onChainInitialized) {
        const result = await callProgram("reset");

        if (result && result.success) {
          const previousValue = state.onChainValue;
          state.onChainValue = 0;
          state.lastSlot = result.slot;

          state.transactions.unshift({
            signature: result.signature,
            type: "Reset",
            success: true,
            slot: result.slot,
            computeUnits: result.computeUnits,
          });

          if (result.events) {
            state.events.unshift(...result.events);
          }

          updateDisplay();
          saveState();
          setLastAction(
            `Reset from ${previousValue} to 0 (slot ${result.slot})`,
          );
        }
      } else {
        const previousValue = state.localValue;
        state.localValue = 0;
        updateDisplay();
        saveState();
        setLastAction(`Reset from ${previousValue} to 0 (local)`);
      }

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("medium");
      }
    } catch (e) {
      console.error("Reset failed:", e);
      setLastAction(`Failed: ${e.message}`);
    } finally {
      elements.btnReset.disabled = false;
    }
  }

  async function setValue(newValue) {
    try {
      if (state.onChainInitialized) {
        const result = await callProgram("set", { value: newValue });

        if (result && result.success) {
          const previousValue = state.onChainValue;
          state.onChainValue = newValue;
          state.lastSlot = result.slot;

          state.transactions.unshift({
            signature: result.signature,
            type: "Set",
            success: true,
            slot: result.slot,
            computeUnits: result.computeUnits,
          });

          if (result.events) {
            state.events.unshift(...result.events);
          }

          updateDisplay();
          saveState();
          setLastAction(
            `Set from ${previousValue} to ${newValue} (slot ${result.slot})`,
          );
        }
      } else {
        const previousValue = state.localValue;
        state.localValue = newValue;
        updateDisplay();
        saveState();
        setLastAction(`Set from ${previousValue} to ${newValue} (local)`);
      }

      if (window.dchat?.ui) {
        window.dchat.ui.haptic("success");
      }
    } catch (e) {
      console.error("Failed to set value:", e);
      setLastAction(`Failed: ${e.message}`);
    }
  }

  // ========================================
  // Persistence
  // ========================================

  async function loadState() {
    if (window.dchat?.storage) {
      try {
        const saved = await window.dchat.storage.get("counter_state");
        if (saved) {
          state = { ...state, ...saved };
          updateDisplay();
          return;
        }
      } catch (e) {
        console.debug("dchat storage not available:", e.message);
      }
    }

    try {
      const saved = localStorage.getItem("counter_state");
      if (saved) {
        state = { ...state, ...JSON.parse(saved) };
        updateDisplay();
      }
    } catch (e) {
      console.debug("localStorage not available:", e.message);
    }
  }

  async function saveState() {
    const saveData = {
      localValue: state.localValue,
      totalIncrements: state.totalIncrements,
      totalDecrements: state.totalDecrements,
    };

    if (window.dchat?.storage) {
      try {
        await window.dchat.storage.set("counter_state", saveData);
        return;
      } catch (e) {
        console.debug("dchat storage not available:", e.message);
      }
    }

    try {
      localStorage.setItem("counter_state", JSON.stringify(saveData));
    } catch (e) {
      console.debug("localStorage not available:", e.message);
    }
  }

  // ========================================
  // Event Handlers
  // ========================================

  function setupEventListeners() {
    elements.btnIncrement.addEventListener("click", increment);
    elements.btnDecrement.addEventListener("click", decrement);
    elements.btnReset.addEventListener("click", reset);
    elements.btnSet.addEventListener("click", showModal);

    elements.modalCancel.addEventListener("click", hideModal);
    elements.modalConfirm.addEventListener("click", () => {
      const value = parseInt(elements.setValueInput.value, 10);
      if (!isNaN(value)) {
        setValue(value);
      }
      hideModal();
    });

    elements.setValueInput.addEventListener("keypress", (e) => {
      if (e.key === "Enter") {
        elements.modalConfirm.click();
      }
    });

    elements.modalOverlay.addEventListener("click", (e) => {
      if (e.target === elements.modalOverlay) {
        hideModal();
      }
    });

    document.addEventListener("keydown", (e) => {
      if (elements.modalOverlay.classList.contains("hidden")) {
        switch (e.key) {
          case "ArrowUp":
          case "+":
          case "=":
            increment();
            break;
          case "ArrowDown":
          case "-":
            decrement();
            break;
          case "r":
          case "R":
            if (!e.ctrlKey && !e.metaKey) {
              reset();
            }
            break;
          case "s":
          case "S":
            if (!e.ctrlKey && !e.metaKey) {
              showModal();
            }
            break;
        }
      } else if (e.key === "Escape") {
        hideModal();
      }
    });
  }

  // ========================================
  // dchat SDK Integration
  // ========================================

  function setupDchatIntegration() {
    if (!window.dchat) {
      setStatus("Standalone mode", "connected");
      return;
    }

    window.dchat.on("ready", (data) => {
      setStatus("Connected", "connected");
      console.info("dchat ready:", data);

      if (window.dchat.analytics) {
        window.dchat.analytics.track("app_opened", {
          initialValue: state.localValue,
        });
      }
    });

    window.dchat.on("userUpdate", (user) => {
      console.info("User updated:", user);
    });

    window.dchat.on("themeUpdate", (newTheme) => {
      console.info("Theme changed:", newTheme);
    });

    if (window.dchat.isReady) {
      setStatus("Connected", "connected");
    }
  }

  // ========================================
  // Initialization
  // ========================================

  async function init() {
    console.info("🔢 Counter Mini-App with On-Chain Integration starting...");

    setupEventListeners();
    setupDchatIntegration();
    createOnChainPanel();
    await loadState();

    updateDisplay();
    console.info("🔢 Counter Mini-App ready!");
    console.info("   - Use buttons or keyboard (↑/↓, +/-, r, s)");
    console.info("   - Click 'Initialize On-Chain Counter' to deploy to chain");
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }
})();
