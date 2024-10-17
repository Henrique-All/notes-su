// Import the functions you need from the SDKs you need
import { initializeApp } from 'firebase/app'
import { getFirestore } from 'firebase/firestore'
// TODO: Add SDKs for Firebase products that you want to use
// https://firebase.google.com/docs/web/setup#available-libraries

// Your web app's Firebase configuration
// For Firebase JS SDK v7.20.0 and later, measurementId is optional
const firebaseConfig = {
  apiKey: 'AIzaSyCUbILfS-rnp7DwK9W4mEv2n2SauX085Ag',
  authDomain: 'notesnu-2d27b.firebaseapp.com',
  projectId: 'notesnu-2d27b',
  storageBucket: 'notesnu-2d27b.appspot.com',
  messagingSenderId: '476227763033',
  appId: '1:476227763033:web:74aa3c78d305460b06ea48',
  measurementId: 'G-5QB0L164TL',
}

// Initialize Firebase
const app = initializeApp(firebaseConfig)
export const db = getFirestore(app)
