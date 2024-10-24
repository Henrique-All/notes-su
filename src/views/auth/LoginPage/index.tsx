import React from 'react'
import Logo from '../../../assets/logoSU.png'
import { ButtonAuthCommon } from '../../../styles/CommonsComponents'
import { LoginContent } from './styles'
import { Link } from 'react-router-dom'

function LoginPage() {
  return (
    <LoginContent>
      <div className='left'>
        <img src={Logo} alt='' />
      </div>

      <div className='right'>
        <p>LOGIN</p>
        <div className='email'>
          <label htmlFor=''>Email:</label>
          <input type='text' id='email' autoComplete='new-password' />
        </div>
        <div className='password'>
          <label htmlFor=''>Senha:</label>
          <input type='password' id='password' autoComplete='new-password' />
        </div>
        <ButtonAuthCommon type='button'>Entrar</ButtonAuthCommon>
        <div className='bottom'>
          <Link to='/cadastro'>Novo aqui? Clique aqui e faça o seu cadastro.</Link>
          <Link to='/esqueci-a-senha'>Esqueceu sua senha?</Link>
        </div>
      </div>
    </LoginContent>
  )
}

export default LoginPage
