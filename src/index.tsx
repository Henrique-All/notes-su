import React from 'react'
import ReactDOM from 'react-dom/client'
import GlobalStyles from './styles/GlobalStyles'
import { BrowserRouter } from 'react-router-dom'
import { ToastContainer } from 'react-toastify'
import Router from './routes'

const root = ReactDOM.createRoot(document.getElementById('root') as HTMLElement)
root.render(
  <BrowserRouter>
    <Router />
    <ToastContainer autoClose={1500} className='toast-container' />
    <GlobalStyles />
  </BrowserRouter>,
)
