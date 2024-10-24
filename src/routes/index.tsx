import React from 'react'
import { Route, Routes } from 'react-router-dom'
import HomePage from '../views/home'
import LoginPage from '../views/auth/LoginPage'
import CreateAccount from '../views/auth/CreateAccount'
import RecoveryPassword from '../views/auth/RecoveryPassword'
import NewPassword from '../views/auth/RecoveryPassword/NewPassword'

function Router() {
  return (
    <Routes>
      <Route path='/' element={<LoginPage />} />
      <Route path='/home' element={<HomePage />} />
      <Route path='/cadastro' element={<CreateAccount />} />
      <Route path='/esqueci-a-senha' element={<RecoveryPassword />} />
      <Route path='/nova-senha' element={<NewPassword />} />
    </Routes>
  )
}
export default Router
