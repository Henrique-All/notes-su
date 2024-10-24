import React from 'react'
import { AccountContent } from './styles'
import Logo from '../../../assets/logoSU.png'
import { ButtonAuthCommon } from '../../../styles/CommonsComponents'

function CreateAccount() {
  return (
    <AccountContent>
      <div className='left'>
        <img src={Logo} alt='' />
      </div>

      <div className='right'>
        <p>Crie uma nova conta</p>
        <div className='email'>
          <label htmlFor=''>Nome:</label>
          <input type='text' id='email' autoComplete='new-password' />
        </div>
        <div className='password'>
          <label htmlFor=''>Email:</label>
          <input type='password' id='password' autoComplete='new-password' />
        </div>
        <div className='password'>
          <label htmlFor=''>Senha:</label>
          <input type='password' id='password' autoComplete='new-password' />
        </div>
        <div className='password'>
          <label htmlFor=''>Confirme sua Senha:</label>
          <input type='password' id='password' autoComplete='new-password' />
        </div>
        <div className='bottom'>
          <input type='checkbox' required id='check' />
          <label htmlFor='check'>Aceito os termos de uso</label>
        </div>
        <ButtonAuthCommon type='button'>Cadastrar</ButtonAuthCommon>
      </div>
    </AccountContent>
  )
}

export default CreateAccount
