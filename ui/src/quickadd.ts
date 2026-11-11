import { mount } from "svelte";
import "./styles/theme.css";
import QuickAddApp from "./QuickAddApp.svelte";

mount(QuickAddApp, { target: document.getElementById("app")! });
