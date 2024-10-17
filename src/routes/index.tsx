import React from 'react'
import { Route, Routes } from 'react-router-dom'
import HomePage from '../views/home'

function Router() {
  return (
    <Routes>
      <Route path='/' element={<HomePage />} />
    </Routes>
  )
}
export default Router
