import '../../app.css';
import { mount } from 'svelte';
import LoadingVariants from './LoadingVariants.svelte';
const driver = mount(LoadingVariants, { target: document.getElementById('app')! });
(window as unknown as { loadingValidation: typeof driver }).loadingValidation = driver;
