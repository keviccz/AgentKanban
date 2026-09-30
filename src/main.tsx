import React from 'react';
import ReactDOM from 'react-dom/client';
import { App } from './App';
import { windowLabel } from './bridge';
import { PetApp } from './pet/Pet';
import './styles.css';

// One bundle, two windows: the board, and the transparent desktop pet window.
const pet = windowLabel().startsWith('pet');
if (pet) document.documentElement.classList.add('pet-window');
ReactDOM.createRoot(document.getElementById('root')!).render(<React.StrictMode>{pet ? <PetApp /> : <App />}</React.StrictMode>);
